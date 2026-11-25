//! End-to-end stdio test: spawn the real binary and speak JSON-RPC to it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use mappamente_core::MindMap;
use mappamente_core::io::json;
use serde_json::{Value, json};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "mappamente-mcp-stdio-{label}-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct ServerProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl ServerProcess {
    fn start(directory: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mappamente-mcp"))
            .arg("--dir")
            .arg(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mappamente-mcp");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        Self {
            child,
            stdin: Some(stdin),
            stdout,
            next_id: 0,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{message}").expect("write request");
        stdin.flush().expect("flush request");
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn read_message(&mut self) -> Value {
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read stdout");
            assert!(read > 0, "server closed stdout");
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            return serde_json::from_str(line).expect("valid JSON-RPC line");
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id();
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        loop {
            let message = self.read_message();
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                assert!(message.get("error").is_none(), "protocol error: {message}");
                return message["result"].clone();
            }
        }
    }

    fn initialize(&mut self) {
        let result = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "mappamente-test", "version": "1.0.0" },
            }),
        );
        assert_eq!(result["serverInfo"]["name"], "mappamente-mcp");
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        }));
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let structured = match result.get("structuredContent") {
            Some(value) => value.clone(),
            None => {
                let text = result["content"][0]["text"].as_str().unwrap_or("null");
                serde_json::from_str(text).expect("tool text is JSON")
            }
        };
        (is_error, structured)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_seed_map(directory: &Path) -> String {
    let map = MindMap::new("Seed", "Root");
    let root_id = map.root.id.clone();
    json::save(&directory.join("seed.mappa"), &map).unwrap();
    root_id
}

#[test]
fn stdio_server_lists_tools_and_runs_the_full_lifecycle() {
    let directory = TempDir::new("lifecycle");
    let seed_root = write_seed_map(directory.path());

    let mut server = ServerProcess::start(directory.path());
    server.initialize();

    let tools = server.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(names.len(), 10, "unexpected tools: {names:?}");
    for expected in [
        "list_maps",
        "get_map",
        "create_map",
        "add_node",
        "add_subtree_from_markdown",
        "update_node",
        "move_node",
        "delete_node",
        "set_layout",
        "export",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    let (is_error, listing) = server.call_tool("list_maps", json!({}));
    assert!(!is_error, "list_maps failed: {listing}");
    let maps = listing["maps"].as_array().unwrap();
    assert_eq!(maps.len(), 1);
    assert_eq!(maps[0]["name"], "seed.mappa");

    let (is_error, document) = server.call_tool("get_map", json!({ "map": "seed" }));
    assert!(!is_error, "get_map failed: {document}");
    assert_eq!(document["root_id"], seed_root);
    assert!(document["outline"].as_str().unwrap().contains("Root"));
    let seed_modified_at = document["modified_at"].as_u64().unwrap();

    let (is_error, created) = server.call_tool(
        "create_map",
        json!({ "map": "new-map", "title": "MCP", "root": "Center" }),
    );
    assert!(!is_error, "create_map failed: {created}");
    assert_eq!(created["map"]["name"], "new-map.mappa");
    assert_eq!(created["map"]["layout"], "radial");
    let root_id = created["outline"]
        .as_str()
        .unwrap()
        .split('[')
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("root id")
        .to_owned();

    let (is_error, added) = server.call_tool(
        "add_node",
        json!({
            "map": "new-map",
            "parent_id": root_id,
            "text": "Child",
            "expected_modified_at": created["modified_at"],
        }),
    );
    assert!(!is_error, "add_node failed: {added}");
    assert_eq!(added["node_ids"].as_array().unwrap().len(), 1);
    assert!(added["outline"].as_str().unwrap().contains("Child"));
    let current = added["modified_at"].as_u64().unwrap();
    assert!(current > created["modified_at"].as_u64().unwrap());

    let (is_error, conflict) = server.call_tool(
        "add_node",
        json!({
            "map": "new-map",
            "parent_id": root_id,
            "text": "Stale",
            "expected_modified_at": created["modified_at"],
        }),
    );
    assert!(is_error);
    assert_eq!(conflict["error"]["code"], "modified_at_mismatch");

    let (is_error, subtree) = server.call_tool(
        "add_subtree_from_markdown",
        json!({
            "map": "new-map",
            "parent_id": root_id,
            "markdown": "- Heading\n  - Alpha\n    - Detail\n  - Beta\n",
            "expected_modified_at": current,
        }),
    );
    assert!(!is_error, "add_subtree failed: {subtree}");
    assert_eq!(subtree["node_ids"].as_array().unwrap().len(), 2);
    assert_eq!(subtree["node_count"], 5);

    let (is_error, updated) = server.call_tool(
        "update_node",
        json!({
            "map": "new-map",
            "node_id": root_id,
            "note": "Server root",
            "color": "#ff8800",
            "collapsed": true,
            "expected_modified_at": subtree["modified_at"],
        }),
    );
    assert!(!is_error, "update_node failed: {updated}");
    assert!(
        updated["outline"]
            .as_str()
            .unwrap()
            .contains("note=\"Server root\"")
    );

    let (is_error, layout) = server.call_tool(
        "set_layout",
        json!({
            "map": "new-map",
            "layout": "tree",
            "expected_modified_at": updated["modified_at"],
        }),
    );
    assert!(!is_error, "set_layout failed: {layout}");
    assert_eq!(layout["map"]["layout"], "tree");

    let (is_error, exported) =
        server.call_tool("export", json!({ "map": "new-map", "format": "svg" }));
    assert!(!is_error, "export failed: {exported}");
    assert_eq!(exported["exported"], "new-map.svg");
    let svg = std::fs::read_to_string(directory.path().join("new-map.svg")).unwrap();
    assert!(svg.starts_with("<?xml"));

    let (is_error, deleted) = server.call_tool(
        "delete_node",
        json!({
            "map": "new-map",
            "node_id": subtree["node_ids"][0],
            "expected_modified_at": layout["modified_at"],
        }),
    );
    assert!(!is_error, "delete_node failed: {deleted}");
    assert_eq!(deleted["node_count"], 3);

    let (is_error, escaped) = server.call_tool("get_map", json!({ "map": "../seed.mappa" }));
    assert!(is_error);
    assert_eq!(escaped["error"]["code"], "invalid_map_name");

    let (is_error, missing) = server.call_tool("get_map", json!({ "map": "absent" }));
    assert!(is_error);
    assert_eq!(missing["error"]["code"], "map_not_found");

    let (is_error, stale_seed) = server.call_tool(
        "update_node",
        json!({
            "map": "seed",
            "node_id": seed_root,
            "text": "Changed",
            "expected_modified_at": seed_modified_at - 1,
        }),
    );
    assert!(is_error);
    assert_eq!(stale_seed["error"]["code"], "modified_at_mismatch");
}

#[test]
fn binary_rejects_a_missing_directory_without_writing_to_stdout() {
    let directory = TempDir::new("missing");
    let missing = directory.path().join("absent");
    let output = Command::new(env!("CARGO_BIN_EXE_mappamente-mcp"))
        .arg("--dir")
        .arg(&missing)
        .output()
        .expect("run mappamente-mcp");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not exist"),
        "unexpected stderr: {stderr}"
    );
    assert!(output.stdout.is_empty());
}
