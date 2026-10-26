use fdxmp_core::color_engine::ColorStage;
use fdxmp_core::params::{EditParams, GradingZone};
fn main() {
 let mut params=EditParams::default(); params.grading.midtones=GradingZone{hue:120.0,saturation:100.0,luminance:0.0};
 println!("params {:?}",params.grading);
 let out=ColorStage::from_params(&params).apply([0.3,0.3,0.3]); println!("out {out:?}");
 let lab=fdxmp_color::oklab_from_rgb(&fdxmp_core::color_engine::WORKSPACE,out); println!("lab {lab:?}");
}
