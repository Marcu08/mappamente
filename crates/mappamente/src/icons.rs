//! Small vector icons drawn with the egui painter.
//!
//! No icon font or extra dependency: icons scale with the UI and inherit the
//! current token color.  Each icon is designed on a 24x24 logical grid so it
//! stays crisp, symmetric and visually balanced at toolbar sizes.

use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

/// Toolbar icon kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Fit,
    Ai,
    Layout,
    Theme,
    Grid,
}

/// Draws `icon` centered in `rect` using `color`.
pub(crate) fn draw(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let size = rect.width().min(rect.height());
    let scale = size / 24.0;
    let origin = rect.center() - Vec2::splat(size * 0.5);
    let stroke_width = (1.85 * scale).max(1.5);
    let stroke = Stroke::new(stroke_width, color);
    let ctx = IconCtx {
        painter,
        origin,
        scale,
        stroke,
        color,
    };

    match icon {
        Icon::New => ctx.new_document(),
        Icon::Open => ctx.open_folder(),
        Icon::Save => ctx.floppy_disk(),
        Icon::SaveAs => ctx.save_as(),
        Icon::Undo => ctx.undo(),
        Icon::Redo => ctx.redo(),
        Icon::Fit => ctx.fit(),
        Icon::Ai => ctx.ai(),
        Icon::Layout => ctx.layout(),
        Icon::Theme => ctx.theme(),
        Icon::Grid => ctx.grid(),
    }
}

struct IconCtx<'p> {
    painter: &'p Painter,
    origin: Pos2,
    scale: f32,
    stroke: Stroke,
    color: Color32,
}

impl<'p> IconCtx<'p> {
    /// Convert grid coordinates (0..24) to screen coordinates.
    fn p(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(
            self.origin.x + x * self.scale,
            self.origin.y + y * self.scale,
        )
    }

    fn line(&self, a: Pos2, b: Pos2) {
        self.painter.line_segment([a, b], self.stroke);
    }

    fn circle_fill(&self, center: Pos2, radius: f32) {
        self.painter
            .circle_filled(center, radius * self.scale, self.color);
    }

    fn rect_stroke(&self, rect: Rect, radius: u8) {
        self.painter.rect_stroke(
            rect,
            CornerRadius::same(radius),
            self.stroke,
            StrokeKind::Inside,
        );
    }

    fn rect_fill(&self, rect: Rect, radius: u8) {
        self.painter
            .rect_filled(rect, CornerRadius::same(radius), self.color);
    }

    fn polygon_fill(&self, points: &[Pos2]) {
        self.painter.add(Shape::convex_polygon(
            points.to_vec(),
            self.color,
            Stroke::NONE,
        ));
    }

    fn new_document(&self) {
        // Page outline.
        let page = Rect::from_min_max(self.p(4.0, 2.0), self.p(20.0, 22.0));
        self.rect_stroke(page, 2);
        // Folded corner.
        self.line(self.p(15.0, 2.0), self.p(15.0, 8.0));
        self.line(self.p(15.0, 8.0), self.p(20.0, 8.0));
        // Small plus.
        let cx = page.center().x;
        let cy = page.center().y + 1.0 * self.scale;
        let arm = 2.5 * self.scale;
        self.painter.line_segment(
            [Pos2::new(cx - arm, cy), Pos2::new(cx + arm, cy)],
            self.stroke,
        );
        self.painter.line_segment(
            [Pos2::new(cx, cy - arm), Pos2::new(cx, cy + arm)],
            self.stroke,
        );
    }

    fn open_folder(&self) {
        // Folder tab and body drawn as a single open folder silhouette.
        let tab_left = self.p(3.0, 6.0);
        let tab_top = self.p(9.0, 6.0);
        let tab_peak = self.p(12.0, 4.0);
        let tab_right = self.p(17.0, 4.0);
        let body_top_left = self.p(3.0, 9.0);
        let body_top_right = self.p(21.0, 9.0);
        let body_bottom_right = self.p(21.0, 19.0);
        let body_bottom_left = self.p(3.0, 19.0);

        self.painter.add(Shape::convex_polygon(
            vec![
                tab_left,
                tab_top,
                tab_peak,
                tab_right,
                body_top_right,
                body_bottom_right,
                body_bottom_left,
                body_top_left,
            ],
            Color32::TRANSPARENT,
            self.stroke,
        ));
    }

    fn floppy_disk(&self) {
        // Classic floppy silhouette with strong, simple shapes.
        let outer = Rect::from_min_max(self.p(4.5, 3.0), self.p(19.5, 21.0));
        self.rect_stroke(outer, 2);
        // Metal shutter at top.
        let shutter = Rect::from_min_max(self.p(7.5, 3.0), self.p(16.5, 9.5));
        self.rect_fill(shutter, 1);
        // Small write-protect notch in the shutter.
        self.rect_fill(Rect::from_min_max(self.p(14.0, 4.0), self.p(15.5, 7.0)), 0);
        // Label area at bottom.
        let label = Rect::from_min_max(self.p(6.0, 12.5), self.p(18.0, 19.5));
        self.rect_stroke(label, 1);
    }

    fn save_as(&self) {
        self.floppy_disk();
        // Clear plus badge bottom-right for "save as".
        let center = self.p(19.5, 18.0);
        let arm = 2.0 * self.scale;
        self.painter.line_segment(
            [
                Pos2::new(center.x - arm, center.y),
                Pos2::new(center.x + arm, center.y),
            ],
            Stroke::new(self.stroke.width * 1.3, self.color),
        );
        self.painter.line_segment(
            [
                Pos2::new(center.x, center.y - arm),
                Pos2::new(center.x, center.y + arm),
            ],
            Stroke::new(self.stroke.width * 1.3, self.color),
        );
    }

    fn undo(&self) {
        self.curved_arrow(-1.0);
    }

    fn redo(&self) {
        self.curved_arrow(1.0);
    }

    fn curved_arrow(&self, direction: f32) {
        // Classic undo/redo curved arrow drawn as a symmetric cubic Bézier.
        // For undo (direction = -1) the tail is on the right and the head
        // points left; for redo (direction = +1) it is mirrored.
        let start = self.p(12.0, 17.0) + Vec2::new(-5.5 * direction, 0.0) * self.scale;
        let end = self.p(12.0, 11.0) + Vec2::new(5.5 * direction, 0.0) * self.scale;
        let control_a = self.p(12.0, 5.0) + Vec2::new(-5.5 * direction, 0.0) * self.scale;
        let control_b = self.p(12.0, 5.0) + Vec2::new(5.5 * direction, 0.0) * self.scale;

        self.painter.add(Shape::CubicBezier(
            eframe::egui::epaint::CubicBezierShape::from_points_stroke(
                [start, control_a, control_b, end],
                false,
                Color32::TRANSPARENT,
                self.stroke,
            ),
        ));

        // Arrow head points horizontally toward the end of the curve.
        let tip = end + Vec2::new(3.5 * direction, 0.0) * self.scale;
        let side = Vec2::new(-2.0 * direction, 2.0) * self.scale;
        self.polygon_fill(&[tip, end + side, end - side]);
    }

    fn fit(&self) {
        // Four corner brackets pointing toward the center.
        let inset = 4.0;
        let arm = 5.5;
        let corners = [
            (
                self.p(inset, inset),
                Vec2::new(arm, 0.0),
                Vec2::new(0.0, arm),
            ),
            (
                self.p(24.0 - inset, inset),
                Vec2::new(-arm, 0.0),
                Vec2::new(0.0, arm),
            ),
            (
                self.p(inset, 24.0 - inset),
                Vec2::new(arm, 0.0),
                Vec2::new(0.0, -arm),
            ),
            (
                self.p(24.0 - inset, 24.0 - inset),
                Vec2::new(-arm, 0.0),
                Vec2::new(0.0, -arm),
            ),
        ];
        for (point, dx, dy) in corners {
            self.line(point, point + dx * self.scale);
            self.line(point, point + dy * self.scale);
        }
    }

    fn ai(&self) {
        // Sparkle icon: two four-point stars, the standard "AI" glyph.
        let big_center = self.p(11.0, 12.5);
        let small_center = self.p(17.0, 7.0);
        self.polygon_fill(&star(
            big_center,
            8.0 * self.scale,
            3.2 * self.scale,
            4,
            -std::f32::consts::FRAC_PI_2,
        ));
        self.polygon_fill(&star(
            small_center,
            4.0 * self.scale,
            1.5 * self.scale,
            4,
            -std::f32::consts::FRAC_PI_2,
        ));
    }

    fn layout(&self) {
        // Three-square org-chart: parent centered, two children below.
        let parent = Rect::from_min_max(self.p(9.0, 3.0), self.p(15.0, 9.0));
        let left_child = Rect::from_min_max(self.p(4.0, 16.0), self.p(10.0, 22.0));
        let right_child = Rect::from_min_max(self.p(14.0, 16.0), self.p(20.0, 22.0));
        self.rect_stroke(parent, 2);
        self.rect_stroke(left_child, 2);
        self.rect_stroke(right_child, 2);
        self.line(self.p(12.0, 9.0), self.p(12.0, 12.0));
        self.line(self.p(7.0, 12.0), self.p(17.0, 12.0));
        self.line(self.p(7.0, 12.0), self.p(7.0, 16.0));
        self.line(self.p(17.0, 12.0), self.p(17.0, 16.0));
    }

    fn theme(&self) {
        // Half-filled circle: left side filled, right side stroked.
        let center = self.p(12.0, 12.0);
        let radius = 8.0;
        // Right semicircle outline.
        let right_points = arc(
            center,
            radius * self.scale,
            -std::f32::consts::FRAC_PI_2,
            std::f32::consts::FRAC_PI_2,
            10,
        );
        for window in right_points.windows(2) {
            self.line(window[0], window[1]);
        }
        self.line(right_points[0], self.p(12.0, 20.0));
        self.line(right_points[right_points.len() - 1], self.p(12.0, 4.0));
        // Diameter.
        self.line(self.p(12.0, 4.0), self.p(12.0, 20.0));
        // Left semicircle filled.
        let mut left_points = arc(
            center,
            radius * self.scale,
            std::f32::consts::FRAC_PI_2,
            3.0 * std::f32::consts::FRAC_PI_2,
            10,
        );
        left_points.push(self.p(12.0, 4.0));
        self.polygon_fill(&left_points);
    }

    fn grid(&self) {
        // 3x3 dot grid, perfectly spaced.
        for row in 0..3 {
            for col in 0..3 {
                let x = 6.0 + col as f32 * 6.0;
                let y = 6.0 + row as f32 * 6.0;
                self.circle_fill(self.p(x, y), 1.25);
            }
        }
    }
}

fn star(
    center: Pos2,
    outer_radius: f32,
    inner_radius: f32,
    points: usize,
    rotation: f32,
) -> Vec<Pos2> {
    let mut result = Vec::with_capacity(points * 2);
    for i in 0..points * 2 {
        let angle = rotation - i as f32 * std::f32::consts::PI / points as f32;
        let radius = if i % 2 == 0 {
            outer_radius
        } else {
            inner_radius
        };
        result.push(Pos2::new(
            center.x + radius * angle.cos(),
            center.y - radius * angle.sin(),
        ));
    }
    result
}

fn arc(center: Pos2, radius: f32, start: f32, end: f32, segments: usize) -> Vec<Pos2> {
    let mut points = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let angle = start + (end - start) * t;
        points.push(Pos2::new(
            center.x + radius * angle.cos(),
            center.y - radius * angle.sin(),
        ));
    }
    points
}
