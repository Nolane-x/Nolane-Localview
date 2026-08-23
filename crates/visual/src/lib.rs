#![forbid(unsafe_code)]

use serde::{Deserialize,Serialize};
use thiserror::Error;

#[derive(Debug,Error)] pub enum VisualError { #[error("image dimensions do not match buffer length")] InvalidBuffer, #[error("images have different dimensions")] DimensionMismatch }
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct RgbaImage { pub width:u32,pub height:u32,pub data:Vec<u8> }
impl RgbaImage { pub fn validate(&self)->Result<(),VisualError>{if self.data.len()!=self.width as usize*self.height as usize*4{return Err(VisualError::InvalidBuffer);}Ok(())} }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)] pub struct DiffRegion { pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub changed_pixels:u64 }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)] pub struct VisualDiff { pub changed_pixels:u64,pub changed_ratio:f64,pub bounding_box:Option<DiffRegion> }

pub fn pixel_diff(before:&RgbaImage,after:&RgbaImage,threshold:u8)->Result<VisualDiff,VisualError>{
    before.validate()?;after.validate()?;if before.width!=after.width||before.height!=after.height{return Err(VisualError::DimensionMismatch);}
    let mut changed=0u64;let(mut min_x,mut min_y)=(u32::MAX,u32::MAX);let(mut max_x,mut max_y)=(0u32,0u32);
    for p in 0..(before.width*before.height) as usize {let i=p*4; let delta=(0..4).map(|c|before.data[i+c].abs_diff(after.data[i+c])).max().unwrap_or(0);if delta>threshold{changed+=1;let x=p as u32%before.width;let y=p as u32/before.width;min_x=min_x.min(x);min_y=min_y.min(y);max_x=max_x.max(x);max_y=max_y.max(y);}}
    let bounding_box=(changed>0).then(||DiffRegion{x:min_x,y:min_y,width:max_x-min_x+1,height:max_y-min_y+1,changed_pixels:changed});
    Ok(VisualDiff{changed_pixels:changed,changed_ratio:changed as f64/(before.width as f64*before.height as f64).max(1.0),bounding_box})
}

pub fn changed_tiles(before:&RgbaImage,after:&RgbaImage,tile:u32,threshold:u8)->Result<Vec<DiffRegion>,VisualError>{
    before.validate()?;after.validate()?;if before.width!=after.width||before.height!=after.height{return Err(VisualError::DimensionMismatch);} let tile=tile.max(1);let mut regions=Vec::new();
    let mut y=0;while y<before.height{let mut x=0;while x<before.width{let w=tile.min(before.width-x);let h=tile.min(before.height-y);let mut changed=0;for yy in y..y+h{for xx in x..x+w{let i=((yy*before.width+xx)*4)as usize;let d=(0..4).map(|c|before.data[i+c].abs_diff(after.data[i+c])).max().unwrap_or(0);if d>threshold{changed+=1;}}}if changed>0{regions.push(DiffRegion{x,y,width:w,height:h,changed_pixels:changed});}x+=tile;}y+=tile;}Ok(regions)
}

#[cfg(test)]mod tests{use super::*;#[test]fn one_pixel_diff_is_local(){let a=RgbaImage{width:2,height:2,data:vec![0;16]};let mut b=a.clone();b.data[4]=255;let d=pixel_diff(&a,&b,1).unwrap();assert_eq!(d.changed_pixels,1);assert_eq!(d.bounding_box.unwrap().x,1);}}
