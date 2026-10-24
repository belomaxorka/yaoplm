//! The loader catalogue through `dispatch` (LDR-1), as the front ends ask
//! for it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use yaoplm_api::Api;

#[test]
fn the_catalogue_is_one_request_away() {
    let api = Api::new();
    let response = api.dispatch_json(r#"{"type":"LoaderCatalog"}"#).unwrap();
    let json: Value = serde_json::to_value(response).unwrap();
    assert_eq!(json["type"], "LoaderCatalog");
    assert_eq!(json["schema"], 1);
    let builds = json["builds"].as_array().unwrap();
    let ids: Vec<&str> = builds.iter().map(|b| b["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["opl-stable", "opl-beta", "yaopl"]);
    let recommended: Vec<&str> = builds
        .iter()
        .filter(|b| b["recommended"] == true)
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(recommended, ["opl-beta"]);
    for build in builds {
        assert!(build["summary"]["en"].is_string() && build["summary"]["ru"].is_string());
        assert_eq!(build["license"]["spdx"], "AFL-3.0");
        let presets: Vec<&str> = build["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["path"].as_str().unwrap())
            .collect();
        assert!(presets.contains(&"UPDT/OPL.ELF"), "{presets:?}");
        assert!(presets.contains(&"APPS/OPNPS2LD.ELF"), "{presets:?}");
    }
    assert_eq!(builds[0]["profile"], "Opl110");
    assert_eq!(builds[0]["channel"]["tag"], "v1.1.0");
    assert_eq!(builds[0]["channel"]["floating"], false);
    assert_eq!(builds[2]["publisher"], "fork");
}
