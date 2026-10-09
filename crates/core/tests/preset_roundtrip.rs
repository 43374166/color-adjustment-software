//! 集成测试：`.fdxmp` 文件读写（磁盘路径）与错误分支。

use std::io::Cursor;

use fdxmp_core::params::{EditParams, MaskKind, MaskParams, SliderId};
use fdxmp_core::preset::{LutAsset, PresetFile, PRESET_SCHEMA_VERSION};

#[test]
fn preset_save_and_load_on_disk() {
    let dir = std::env::temp_dir().join(format!("fdxmp-preset-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.fdxmp");

    let mut params = EditParams::default();
    params.set_slider(SliderId::Contrast, 25.0);
    params.masks.push(MaskParams {
        id: "mask-1".to_string(),
        name: "天空".to_string(),
        kind: MaskKind::AiSky,
        adjustments: Box::new(EditParams::default()),
        ..MaskParams::default()
    });
    let preset = PresetFile::new("风光增强", "fdxmp", params.clone());
    preset.save(&path).unwrap();

    let loaded = PresetFile::load(&path).unwrap();
    assert_eq!(loaded.schema_version, PRESET_SCHEMA_VERSION);
    assert_eq!(loaded.params, params);
    assert_eq!(loaded.params.masks.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_schema_version_reports_error() {
    let mut preset = PresetFile::new("bad", "tester", EditParams::default());
    preset.schema_version = 999;
    let err = preset.to_bytes().unwrap_err();
    assert!(err.to_string().contains("schema_version"));
}

#[test]
fn lut_assets_are_preserved() {
    let mut preset = PresetFile::new("lut", "tester", EditParams::default());
    preset.luts.push(LutAsset {
        entry: "luts/vintage.cube".to_string(),
        data: b"LUT_3D_SIZE 33\n".to_vec(),
    });
    let bytes = preset.to_bytes().unwrap();
    let back = PresetFile::from_bytes(&bytes).unwrap();
    assert_eq!(back.luts[0].entry, "luts/vintage.cube");
    assert_eq!(back.luts[0].data, b"LUT_3D_SIZE 33\n");
}

#[test]
fn corrupt_archive_is_rejected() {
    let cursor = Cursor::new(b"not a zip".to_vec());
    let err = PresetFile::read_from(cursor).unwrap_err();
    assert!(matches!(err, fdxmp_core::error::FdxmpError::Zip(_)));
}
