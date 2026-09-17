#![forbid(unsafe_code)]

pub const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 100_000.0;
pub const MAX_FULL_PAGE_TILES: usize = 128;
pub const MAX_FULL_PAGE_PIXELS: u64 = 64_000_000;
pub const MAX_FULL_PAGE_PNG_BYTES: usize = 128 * 1024 * 1024;

const HORIZONTAL_OVERFLOW_TOLERANCE_CSS_PX: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FullPageTilePlan {
    pub scroll_y_css: f64,
    pub source_start_y_css: f64,
    pub contribution_height_css: f64,
    pub destination_start_y_css: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullPagePlan {
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub tiles: Vec<FullPageTilePlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullPageError {
    InvalidGeometry,
    HorizontalOverflow,
    DocumentTooTall,
    TileLimitExceeded,
}

pub fn plan_full_page(
    document_css: (f64, f64),
    viewport_css: (f64, f64),
) -> Result<FullPagePlan, FullPageError> {
    let (document_css_width, document_css_height) = document_css;
    let (viewport_css_width, viewport_css_height) = viewport_css;

    if !document_css_width.is_finite()
        || !document_css_height.is_finite()
        || !viewport_css_width.is_finite()
        || !viewport_css_height.is_finite()
        || document_css_width <= 0.0
        || document_css_height <= 0.0
        || viewport_css_width <= 0.0
        || viewport_css_height <= 0.0
    {
        return Err(FullPageError::InvalidGeometry);
    }

    if document_css_height > MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT {
        return Err(FullPageError::DocumentTooTall);
    }

    if document_css_width > viewport_css_width + HORIZONTAL_OVERFLOW_TOLERANCE_CSS_PX {
        return Err(FullPageError::HorizontalOverflow);
    }

    let max_scroll = (document_css_height - viewport_css_height).max(0.0);
    let mut offsets = Vec::new();
    offsets.push(0.0);

    if max_scroll > 0.0 {
        let mut next = viewport_css_height;
        while next < max_scroll {
            if offsets.len() >= MAX_FULL_PAGE_TILES {
                return Err(FullPageError::TileLimitExceeded);
            }
            offsets.push(next);
            let advanced = next + viewport_css_height;
            if !advanced.is_finite() || advanced <= next {
                return Err(FullPageError::InvalidGeometry);
            }
            next = advanced;
        }

        if offsets.last().copied() != Some(max_scroll) {
            if offsets.len() >= MAX_FULL_PAGE_TILES {
                return Err(FullPageError::TileLimitExceeded);
            }
            offsets.push(max_scroll);
        }
    }

    let mut tiles = Vec::with_capacity(offsets.len());
    let mut covered_end = 0.0_f64;

    for scroll_y_css in offsets {
        let visible_end = (scroll_y_css + viewport_css_height).min(document_css_height);
        let source_start_y_css = (covered_end - scroll_y_css).max(0.0);
        let contribution_height_css = visible_end - covered_end;

        if !visible_end.is_finite()
            || !source_start_y_css.is_finite()
            || !contribution_height_css.is_finite()
            || contribution_height_css <= 0.0
            || source_start_y_css < 0.0
            || source_start_y_css >= viewport_css_height
        {
            return Err(FullPageError::InvalidGeometry);
        }

        tiles.push(FullPageTilePlan {
            scroll_y_css,
            source_start_y_css,
            contribution_height_css,
            destination_start_y_css: covered_end,
        });
        covered_end = visible_end;
    }

    if (covered_end - document_css_height).abs() > f64::EPSILON * document_css_height.max(1.0) {
        return Err(FullPageError::InvalidGeometry);
    }

    Ok(FullPagePlan {
        document_css_width,
        document_css_height,
        viewport_css_width,
        viewport_css_height,
        tiles,
    })
}
