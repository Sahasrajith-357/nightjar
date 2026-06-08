//! A subtle, static geometric motif drawn behind the masthead — a faint
//! low-poly "constellation" of thin connected lines in the theme accent at
//! very low opacity. Texture you feel more than see.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme};

/// Draws the motif in the given accent color.
pub struct Motif {
    pub accent: Color,
}

impl<Message> canvas::Program<Message> for Motif {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // A handful of points in normalized coords (0..1), placed to form a
        // loose angular mesh. Scaled to the canvas size at draw time.
        let pts_norm: [(f32, f32); 7] = [
            (0.08, 0.30),
            (0.22, 0.72),
            (0.38, 0.18),
            (0.52, 0.60),
            (0.68, 0.28),
            (0.84, 0.70),
            (0.94, 0.36),
        ];
        let pts: Vec<Point> = pts_norm
            .iter()
            .map(|(x, y)| Point::new(x * w, y * h))
            .collect();

        // Connect consecutive points, plus a few cross-links, as thin lines.
        let edges: [(usize, usize); 9] = [
            (0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6),
            (0, 2), (2, 4), (3, 5),
        ];

        let line_color = Color { a: 0.07, ..self.accent };
        let stroke = Stroke::default()
            .with_color(line_color)
            .with_width(1.2);

        for (a, b) in edges {
            let path = Path::line(pts[a], pts[b]);
            frame.stroke(&path, stroke.clone());
        }

        // Small dots at the vertices, slightly more visible.
        let dot_color = Color { a: 0.13, ..self.accent };
        for p in &pts {
            let dot = Path::circle(*p, 2.0);
            frame.fill(&dot, dot_color);
        }

        vec![frame.into_geometry()]
    }
}
