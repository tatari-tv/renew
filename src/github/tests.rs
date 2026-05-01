#![allow(clippy::unwrap_used)]

use super::*;

#[test]
fn test_release_info_find_asset_match() {
    let info = ReleaseInfo {
        tag_name: "v0.5.0".to_string(),
        html_url: "https://github.com/tatari-tv/ccu/releases/tag/v0.5.0".to_string(),
        published_at: "2026-05-01T08:00:00Z".to_string(),
        assets: vec![
            ReleaseAsset {
                name: "ccu-v0.5.0-linux-amd64.tar.gz".to_string(),
                browser_download_url:
                    "https://github.com/tatari-tv/ccu/releases/download/v0.5.0/ccu-v0.5.0-linux-amd64.tar.gz"
                        .to_string(),
            },
            ReleaseAsset {
                name: "ccu-v0.5.0-linux-amd64.tar.gz.sha256".to_string(),
                browser_download_url:
                    "https://github.com/tatari-tv/ccu/releases/download/v0.5.0/ccu-v0.5.0-linux-amd64.tar.gz.sha256"
                        .to_string(),
            },
        ],
    };

    let asset = info.find_asset("ccu-v0.5.0-linux-amd64.tar.gz").unwrap();
    assert_eq!(asset.name, "ccu-v0.5.0-linux-amd64.tar.gz");
}

#[test]
fn test_release_info_find_asset_miss() {
    let info = ReleaseInfo {
        tag_name: "v0.5.0".to_string(),
        html_url: String::new(),
        published_at: String::new(),
        assets: vec![],
    };
    assert!(info.find_asset("ccu-v0.5.0-macos-arm64.tar.gz").is_none());
}

#[test]
fn test_release_info_deserialize() {
    let json = r#"{
        "tag_name": "v0.5.0",
        "html_url": "https://github.com/tatari-tv/ccu/releases/tag/v0.5.0",
        "published_at": "2026-05-01T08:00:00Z",
        "assets": [
            {
                "name": "ccu-v0.5.0-linux-amd64.tar.gz",
                "browser_download_url": "https://github.com/tatari-tv/ccu/releases/download/v0.5.0/ccu-v0.5.0-linux-amd64.tar.gz"
            }
        ]
    }"#;

    let info: ReleaseInfo = serde_json::from_str(json).unwrap();
    assert_eq!(info.tag_name, "v0.5.0");
    assert_eq!(info.assets.len(), 1);
    assert_eq!(info.assets[0].name, "ccu-v0.5.0-linux-amd64.tar.gz");
}
