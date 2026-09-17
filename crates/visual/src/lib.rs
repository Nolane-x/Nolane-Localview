#![forbid(unsafe_code)]

mod full_page;
mod image;

pub use full_page::{
    FullPageError, FullPageOutputGeometry, FullPagePlan, MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT,
    MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT, MAX_FULL_PAGE_OUTPUT_RGBA_BYTES, MAX_FULL_PAGE_TILES,
    plan_full_page, project_full_page_output, stitch_full_page_tile,
};
pub use image::*;
