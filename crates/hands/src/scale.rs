//! Screenshots are shrunk before they go to a model, and the model's coordinates
//! are mapped back to the real screen. Getting this wrong is the classic
//! computer-use bug, so every conversion lives here, tested.

use proto::{Point, Size};

/// The size to send a capture at: the long edge fits `max_edge`, never upscaled.
pub fn fit(screen: Size, max_edge: u32) -> Size {
    let long = screen.width.max(screen.height);
    if long <= max_edge {
        return screen;
    }
    let factor = f64::from(max_edge) / f64::from(long);
    let shrink = |side: u32| ((f64::from(side) * factor).round() as u32).max(1);
    Size {
        width: shrink(screen.width),
        height: shrink(screen.height),
    }
}

/// A point in the image the model saw, on the real screen. `None` when it's
/// outside the image: out-of-bounds clicks are refused, never clamped.
pub fn image_to_screen(point: Point, image: Size, screen: Size) -> Option<Point> {
    let inside = |v: i32, side: u32| v >= 0 && (v as u32) < side;
    if !inside(point.x, image.width) || !inside(point.y, image.height) {
        return None;
    }
    let map = |v: i32, from: u32, to: u32| (f64::from(v) * f64::from(to) / f64::from(from)) as i32;
    Some(Point {
        x: map(point.x, image.width, screen.width),
        y: map(point.y, image.height, screen.height),
    })
}

/// Gemini's computer-use model answers on a 0-999 grid, whatever the image size.
/// Check its docs for the model you use.
pub fn grid_to_screen(point: Point, screen: Size) -> Option<Point> {
    image_to_screen(
        point,
        Size {
            width: 1000,
            height: 1000,
        },
        screen,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn size(width: u32, height: u32) -> Size {
        Size { width, height }
    }

    const fn point(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn fit_shrinks_the_long_edge() {
        assert_eq!(fit(size(2560, 1600), 1280), size(1280, 800));
        assert_eq!(fit(size(3840, 2160), 1280), size(1280, 720));
        assert_eq!(fit(size(1600, 2560), 1280), size(800, 1280));
    }

    #[test]
    fn fit_never_upscales() {
        assert_eq!(fit(size(1024, 768), 1280), size(1024, 768));
    }

    #[test]
    fn image_points_land_on_the_same_spot() {
        let (image, screen) = (size(1280, 800), size(2560, 1600));
        assert_eq!(
            image_to_screen(point(0, 0), image, screen),
            Some(point(0, 0))
        );
        assert_eq!(
            image_to_screen(point(640, 400), image, screen),
            Some(point(1280, 800))
        );
        assert_eq!(
            image_to_screen(point(1279, 799), image, screen),
            Some(point(2558, 1598))
        );
    }

    #[test]
    fn out_of_bounds_is_refused() {
        let (image, screen) = (size(1280, 800), size(2560, 1600));
        assert_eq!(image_to_screen(point(1280, 10), image, screen), None);
        assert_eq!(image_to_screen(point(-1, 10), image, screen), None);
    }

    #[test]
    fn grid_maps_to_the_screen() {
        let screen = size(1920, 1080);
        assert_eq!(
            grid_to_screen(point(500, 500), screen),
            Some(point(960, 540))
        );
        assert_eq!(
            grid_to_screen(point(999, 999), screen),
            Some(point(1918, 1078))
        );
        assert_eq!(grid_to_screen(point(1000, 0), screen), None);
    }
}
