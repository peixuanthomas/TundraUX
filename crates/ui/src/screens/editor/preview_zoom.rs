use ratatui::layout::Rect;

const ZOOM_LEVELS: [u32; 5] = [100, 150, 200, 300, 400];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorPreviewZoom {
    level: usize,
    offset: [Option<u32>; 2],
}

impl EditorPreviewZoom {
    pub fn percent(self) -> u32 {
        ZOOM_LEVELS[self.level]
    }

    pub fn is_zoomed(self) -> bool {
        self.level > 0
    }

    pub fn can_zoom_in(self) -> bool {
        self.level + 1 < ZOOM_LEVELS.len()
    }

    pub fn change(&mut self, zoom_in: bool) {
        let level = if zoom_in {
            (self.level + 1).min(ZOOM_LEVELS.len() - 1)
        } else {
            self.level.saturating_sub(1)
        };
        if self.level != level {
            self.level = level;
            self.center();
        }
    }

    pub fn center(&mut self) {
        self.offset = [None; 2];
    }

    pub(super) fn scale(self, cells: u16) -> u32 {
        (u32::from(cells) * self.percent()).div_ceil(100)
    }

    pub fn pdf_viewport(self, canvas: Rect) -> [u16; 2] {
        [canvas.width, canvas.height].map(|cells| self.scale(cells).min(u32::from(u16::MAX)) as u16)
    }

    pub fn window(self, canvas: Rect, size: [u32; 2]) -> EditorPreviewWindow {
        let [width, height] = [canvas.width, canvas.height];
        let limit = [
            size[0].saturating_sub(u32::from(width)),
            size[1].saturating_sub(u32::from(height)),
        ];
        let offset = |axis: usize| {
            self.offset[axis]
                .unwrap_or(limit[axis] / 2)
                .min(limit[axis])
        };
        let visible_width = size[0].min(u32::from(width)) as u16;
        let visible_height = size[1].min(u32::from(height)) as u16;
        EditorPreviewWindow {
            offset: [offset(0), offset(1)],
            limit,
            area: Rect::new(
                (width - visible_width) / 2,
                (height - visible_height) / 2,
                visible_width,
                visible_height,
            ),
        }
    }

    pub fn pan(&mut self, window: EditorPreviewWindow, [dx, dy]: [i32; 2]) {
        self.pan_to(
            window,
            [
                window.offset[0].saturating_add_signed(dx),
                window.offset[1].saturating_add_signed(dy),
            ],
        );
    }

    pub fn pan_to(&mut self, window: EditorPreviewWindow, offset: [u32; 2]) {
        for (axis, wanted) in offset.into_iter().enumerate() {
            let position = wanted.min(window.limit[axis]);
            if position != window.offset[axis] {
                self.offset[axis] = Some(position);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPreviewWindow {
    pub offset: [u32; 2],
    pub limit: [u32; 2],
    pub area: Rect,
}

pub(super) fn preview_pixel_crop(
    [x, y]: [u32; 2],
    area: Rect,
    [columns, rows]: [u32; 2],
    [width, height]: [u32; 2],
) -> [u32; 4] {
    let axis = |start: u32, cells: u16, total: u32, pixels: u32| {
        let (total, pixels) = (u64::from(total), u64::from(pixels));
        let first = (u64::from(start) * pixels) / total;
        let end = (u64::from(start + u32::from(cells)) * pixels).div_ceil(total);
        (first as u32, (end - first) as u32)
    };
    let (x, width) = axis(x, area.width, columns, width);
    let (y, height) = axis(y, area.height, rows, height);
    [x, y, width, height]
}
