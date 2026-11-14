//! What the generator's tests share: the request `comline generation` sends
//! for a conformance fixture.
#![allow(dead_code)]

use comline_codegen::{connections, facts, GenerateRequest, Mode, PackageMeta, TargetLanguage};
use comline_conformance::Fixture;
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

/// The request for `fx` in `mode`, with the facts its protocols' wire numbers
/// come from, and the package's dependencies.
pub fn request(fx: &Fixture, mode: Mode, async_api: bool) -> GenerateRequest {
    let wire_format = fx
        .wire_format
        .map_or_else(|| "packed@1".to_string(), str::to_string);
    let dependencies: Vec<String> = fx.dependencies.iter().map(|d| d.to_string()).collect();
    let own: Vec<_> = fx
        .schemas
        .iter()
        .filter(|(namespace, _)| !connections::is_dependency(&dependencies, namespace))
        .cloned()
        .collect();
    let mut req = GenerateRequest {
        mode,
        schemas: fx.schemas.clone(),
        package: PackageMeta {
            name: "conformance".into(),
            version: "0.0.0".into(),
        },
        target: TargetLanguage {
            language: "rust".into(),
            version: comline_codegen_rust::LANG_VERSION.into(),
        },
        wire_format: wire_format.clone(),
        external_std: fx.external_std,
        validate_on_decode: fx.validate_on_decode,
        async_api,
        encodings: comline_codegen::encodings::official_for("rust"),
        facts: None,
        dependencies,
    };
    if facts::needed_in(&req) {
        req.facts = Some(comline_conformance::facts_for(&own, &wire_format));
    }
    req
}

/// `fx`'s protocols moved off a connection onto the lockstep `Client` /
/// `Server` (`@framing = "datagram"`).
pub fn lockstep(mut fx: Fixture) -> Fixture {
    for (_, units) in &mut fx.schemas {
        for unit in units.iter_mut() {
            if let FrozenUnit::Protocol { parameters, .. } = unit {
                parameters.push(FrozenUnit::Property {
                    name: "framing".into(),
                    value: ConfigValue::Str("datagram".into()),
                });
            }
        }
    }
    fx
}
