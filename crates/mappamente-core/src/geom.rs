//! Small, dependency-free geometry types shared by layout, GUI and exporters.

/// A point in canvas space (y grows downwards).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Width and height.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    #[must_use]
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

/// An axis-aligned rectangle. `x`/`y` are the top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn from_center(center: Point, size: Size) -> Self {
        Self {
            x: center.x - size.w / 2.0,
            y: center.y - size.h / 2.0,
            w: size.w,
            h: size.h,
        }
    }

    #[must_use]
    pub fn left(self) -> f32 {
        self.x
    }

    #[must_use]
    pub fn top(self) -> f32 {
        self.y
    }

    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    #[must_use]
    pub fn center(self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    #[must_use]
    pub fn origin(self) -> Point {
        Point::new(self.x, self.y)
    }

    /// Smallest rectangle containing both inputs.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }

    /// Returns `true` when the two rectangles share interior area.
    ///
    /// Rectangles that only touch on an edge are not considered overlapping.
    /// `epsilon` shrinks both boxes before the test to tolerate float noise.
    #[must_use]
    pub fn overlaps(self, other: Self, epsilon: f32) -> bool {
        self.x < other.right() - epsilon
            && other.x < self.right() - epsilon
            && self.y < other.bottom() - epsilon
            && other.y < self.bottom() - epsilon
    }

    /// Returns `true` when `other` is fully inside `self` (with optional margin).
    #[must_use]
    pub fn contains_rect(self, other: Self, margin: f32) -> bool {
        other.x >= self.x + margin
            && other.y >= self.y + margin
            && other.right() <= self.right() - margin
            && other.bottom() <= self.bottom() - margin
    }

    /// Point on the border of this rectangle in the direction of `target`.
    ///
    /// Used to attach edges to node boxes instead of their centers. Returns the
    /// center when `target` coincides with it.
    #[must_use]
    pub fn edge_toward(self, target: Point) -> Point {
        let center = self.center();
        let dx = target.x - center.x;
        let dy = target.y - center.y;
        if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
            return center;
        }

        let half_w = self.w / 2.0;
        let half_h = self.h / 2.0;
        let scale_x = if dx.abs() < f32::EPSILON {
            f32::INFINITY
        } else {
            half_w / dx.abs()
        };
        let scale_y = if dy.abs() < f32::EPSILON {
            f32::INFINITY
        } else {
            half_h / dy.abs()
        };
        let scale = scale_x.min(scale_y);
        Point::new(center.x + dx * scale, center.y + dy * scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_and_contains() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, -5.0, 5.0, 5.0);
        let u = a.union(b);
        assert_eq!(u, Rect::new(0.0, -5.0, 25.0, 15.0));
        assert!(u.contains_rect(a, 0.0));
        assert!(u.contains_rect(b, 0.0));
    }

    #[test]
    fn touching_rects_do_not_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert!(!a.overlaps(b, 0.01));
    }

    #[test]
    fn edge_toward_hits_border() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        let right = rect.edge_toward(Point::new(500.0, 20.0));
        assert!((right.x - 100.0).abs() < 1e-6);
        assert!((right.y - 20.0).abs() < 1e-6);

        let bottom = rect.edge_toward(Point::new(50.0, 400.0));
        assert!((bottom.x - 50.0).abs() < 1e-6);
        assert!((bottom.y - 40.0).abs() < 1e-6);

        assert_eq!(rect.edge_toward(rect.center()), rect.center());
    }
}
