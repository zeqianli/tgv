use crate::models::contig::Contig;
use ratatui::layout::Rect;
#[derive(Clone)]
pub struct ViewingWindow {
    pub contig: Contig,

    /// Left most genome coordinate displayed on the screen.
    /// 1-based, inclusive.
    left: usize,

    /// Top track # displayed on the screen.
    /// 0-based.
    top: usize,

    /// Horizontal zoom.
    zoom: usize,
}

impl ViewingWindow {
    pub fn new_basewise_window(contig: Contig, left: usize, top: usize) -> Self {
        Self {
            contig,
            left,
            top,
            zoom: 1,
        }
    }

    pub fn new_zoom_out_window(contig: Contig, left: usize, top: usize, zoom: usize) -> Self {
        Self {
            contig,
            left,
            top,
            zoom,
        }
    }

    pub fn is_basewise(&self) -> bool {
        self.zoom == 1
    }
}

pub enum OnScreenCoordinate {
    /// Coordinate on left side of the screen.
    /// The last pixel is 1.
    Left(usize),

    /// Coordinate on screen.
    /// First pixel is 0.
    OnScreen(usize),

    /// Coordinate on right side of the screen.
    /// The first pixel is 1.
    Right(usize),
}

/// Horizontal coordinates
impl ViewingWindow {
    /// Left genome coordinate of the viewing window.
    /// 1-based, inclusive.
    pub fn left(&self) -> usize {
        self.left
    }

    /// Set the left genome coordinate of the viewing window.
    /// 1-based, inclusive.
    pub fn set_left(&mut self, left: usize) {
        self.left = usize::max(left, 1);
    }

    /// Set the middle genome coordinate of the viewing window.
    /// 1-based, inclusive.
    pub fn set_middle(&mut self, area: &Rect, middle: usize) {
        let left = middle.saturating_sub(self.width(area) / 2);
        self.set_left(left);
    }

    /// Set the top track # of the viewing window.
    /// 0-based.
    pub fn set_top(&mut self, top: usize) {
        self.top = top;
    }

    /// Right genome coordinate of the viewing window.
    /// 1-based, inclusive.
    pub fn right(&self, area: &Rect) -> usize {
        self.left + self.width(area) - 1
    }

    /// Middle genome coordinate of the viewing window.
    /// 1-based, inclusive.
    /// If there is an even number of bases, this calculates the right to the middle.
    pub fn middle(&self, area: &Rect) -> usize {
        self.left + self.width(area) / 2
    }

    /// Width (in bases) of the viewing window.
    pub fn width(&self, area: &Rect) -> usize {
        area.width as usize * self.zoom
    }

    /// Horizontal zoom.
    pub fn zoom(&self) -> usize {
        self.zoom
    }

    /// Check if the viewing window overlaps with [left, right].
    /// 1-based, inclusive.
    pub fn overlaps_x_interval(&self, left: usize, right: usize, area: &Rect) -> bool {
        left <= self.right(area) && right >= self.left()
    }

    /// Returns the onscreen x coordinate in the area. Example
    /// Bases displayed in the window: 1 2 | 3 4 5 6 7 8 | 9 10
    /// Zoom = 2, window has 3 pixels
    /// 1/2 -> Left(0)
    /// 3/4 -> OnScreen(0)
    /// 5/6 -> OnScreen(1)
    /// 7/8 -> OnScreen(2)
    /// 9/10 -> Right(1)
    ///
    /// x: 1-based
    pub fn onscreen_x_coordinate(&self, x: usize, area: &Rect) -> OnScreenCoordinate {
        let self_left = self.left();
        let self_right = self.right(area);

        if x < self_left {
            OnScreenCoordinate::Left(usize::max((self_left - x) / self.zoom, 1))
        } else if x > self_right {
            OnScreenCoordinate::Right(usize::max((x - self_right) / self.zoom, 1))
        } else {
            OnScreenCoordinate::OnScreen((x - self_left) / self.zoom)
        }
    }
}

/// Vertical coordinates
impl ViewingWindow {
    /// Top track # of the viewing window.
    /// 0-based, inclusive.
    pub fn top(&self) -> usize {
        self.top
    }

    /// Bottom track # of the viewing window.
    /// 0-based, exclusive.
    pub fn bottom(&self, area: &Rect) -> usize {
        self.top + self.height(area)
    }

    /// Height of the viewing window.
    pub fn height(&self, area: &Rect) -> usize {
        area.height as usize
    }

    /// Check if the viewing window overlaps with [top, bottom).
    /// y: 0-based.
    pub fn overlaps_y(&self, y: usize, area: &Rect) -> bool {
        self.top <= y && self.bottom(area) > y
    }

    /// Returns the onscreen y coordinate in the area. Example
    /// y: 0-based.
    pub fn onscreen_y_coordinate(&self, y: usize, area: &Rect) -> OnScreenCoordinate {
        let self_top = self.top();
        let self_bottom = self.bottom(area);

        if y < self_top {
            OnScreenCoordinate::Left(self_top - y)
        } else if y >= self_bottom {
            OnScreenCoordinate::Right(y - self_bottom) // Note that this is different from the x coordinate. TODO: think about this.
        } else {
            OnScreenCoordinate::OnScreen(y - self_top)
        }
    }
}

/// Zoom
impl ViewingWindow {
    const MAX_ZOOM: usize = 32; // Temporary. TODO: Improve in the future.

    /// Horizontal zoom out by a factor of r (1-based).
    pub fn zoom_out(&mut self, r: usize, area: &Rect) -> Result<(), ()> {
        if r == 0 {
            return Err(());
        }
        if r == 1 {
            return Ok(());
        }

        let middle_before_zoom = self.middle(area);
        self.zoom = usize::min(self.zoom * r, Self::MAX_ZOOM);
        self.set_middle(area, middle_before_zoom);
        Ok(())
    }

    /// Horizontal zoom in by a factor of r (1-based).
    pub fn zoom_in(&mut self, r: usize, area: &Rect) -> Result<(), ()> {
        if r == 0 {
            return Err(());
        }
        if r == 1 || self.is_basewise() {
            return Ok(());
        }

        let middle_before_zoom = self.middle(area);

        self.zoom = usize::max(self.zoom / r, 1);
        self.set_middle(area, middle_before_zoom);
        Ok(())
    }
}
