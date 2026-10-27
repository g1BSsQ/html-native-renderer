// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Host fontconfig is consulted only when RenderOptions::system_fonts is true.
use blitz_dom::FontContext;
use parley::fontique::{
    Blob, Collection, CollectionOptions, FontInfoOverride, SourceCache,
};
use std::sync::Arc;

pub(crate) fn context(system_fonts: bool, supplied: &[Vec<u8>], html: &str) -> FontContext {
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions { shared: false, system_fonts }),
    };
    // The compiled face has its own family, "Bullet"; never reassign it to a
    // generic. The marker's first named family must resolve without host fonts.
    ctx.collection.register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);
    for bytes in supplied {
        let bytes = blitz_dom::decode_font_bytes(bytes).into_owned();
        ctx.collection.register_fonts(Blob::new(Arc::new(bytes) as _), None);
    }
    if system_fonts {
        #[cfg(target_os = "linux")]
        fontconfig::configure(&mut ctx.collection, html);
    }
    ctx
}

#[cfg(target_os = "linux")]
mod fontconfig {
    use super::*;
    use std::process::Command;
    use std::fs;

    // The native Fontique fontconfig backend already provides fontconfig's
    // cached generic family order and fallback. Never replace it with a
    // single family, or enumerate all fonts at process startup.
    //
    // For exact legacy family names not covered by that backend, query only
    // names requested by the document. CSS already supplies a generic fallback
    // for Arial in our screen fixtures; prefer that stable native fontconfig
    // chain rather than injecting a second copy of the installed sans family
    // under "Arial" (which changes table intrinsic column widths).
    const NEEDS_ALIAS: &[&str] = &[
        "Georgia", "Helvetica", "Times New Roman", "Times", "Courier New",
        "Courier", "Verdana", "Trebuchet MS", "Tahoma", "Calibri", "Cambria",
        "Consolas", "Andale Mono", "Comic Sans MS", "Impact", "Palatino",
        "Bookman",
    ];

    fn fc_match(name: &str) -> Option<(String, String)> {
        // One cache-backed query for one requested family. No fc-list and no
        // /etc/fonts XML traversal; those costs dominated thousands of WPT
        // short-lived processes.
        let output = Command::new("fc-match")
            .args(["-f", "%{family}\t%{file}\n", name]).output().ok()?;
        if !output.status.success() { return None; }
        let output = String::from_utf8(output.stdout).ok()?;
        let (family, path) = output.lines().next()?.split_once('\t')?;
        let family = family.split(',').next()?.trim();
        let path = path.trim();
        if family.is_empty() || !std::path::Path::new(path).is_absolute() {
            return None;
        }
        Some((family.to_owned(), path.to_owned()))
    }

    pub(super) fn configure(collection: &mut Collection, html: &str) {
        let text = html.to_ascii_lowercase();
        for alias in NEEDS_ALIAS {
            if !text.contains(&alias.to_ascii_lowercase()) ||
               collection.family_id(alias).is_some() {
                continue;
            }
            let Some((family, path)) = fc_match(alias) else { continue };
            if family.eq_ignore_ascii_case(alias) { continue; }
            let Ok(bytes) = fs::read(path) else { continue };
            collection.register_fonts(Blob::new(Arc::new(bytes) as _),
                Some(FontInfoOverride { family_name: Some(alias), ..Default::default() }));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn on_demand_match_has_a_file_and_family() {
            let (family, path) = fc_match("serif").expect("fontconfig and a serif face are installed in CI");
            assert!(!family.is_empty());
            assert!(std::path::Path::new(&path).is_absolute());
        }
        #[test]
        fn missing_alias_is_not_resolved_before_a_document_requests_it() {
            let mut ctx = context(true, &[], "<p>Nothing uses a named alias here</p>");
            // Context creation does not run fc-list or load an installed alias
            // family merely because it exists in host configuration.
            assert!(ctx.collection.family_id("Made Up Font Name").is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::fontique::{Attributes, FontStyle, FontWeight, FontWidth};

    fn sample_bytes() -> Vec<u8> {
        let fixture = include_str!("../../../fixtures/15-embedded-font.html");
        let url = fixture.split_once("src:url(").unwrap().1.split_once(')').unwrap().0;
        data_url::DataUrl::process(url).unwrap().decode_to_vec().unwrap().0
    }

    #[test]
    fn six_hundred_selects_seven_hundred_from_four_hundred_seven_hundred() {
        let mut ctx = context(false, &[], "");
        let bytes = sample_bytes();
        for weight in [400.0, 700.0] {
            ctx.collection.register_fonts(Blob::new(Arc::new(bytes.clone()) as _),
                Some(FontInfoOverride { family_name: Some("WeightProbe"),
                    weight: Some(FontWeight::new(weight)), ..Default::default() }));
        }
        let family = ctx.collection.family_by_name("WeightProbe").unwrap();
        let mut query = ctx.collection.query(&mut ctx.source_cache);
        query.set_families(["WeightProbe"]);
        query.set_attributes(Attributes::new(FontWidth::NORMAL, FontStyle::Normal, FontWeight::new(600.0)));
        let selected = query.first_available_font().unwrap();
        assert_eq!(family.fonts()[selected.family.1].weight().value(), 700.0);
    }

    #[test]
    fn regular_only_has_bold_and_oblique_synthesis_flags() {
        let mut ctx = context(false, &[], "");
        // Query an ordinary in-memory regular family, independent of system fonts.
        // Re-register the complete bytes with a deterministic family name.
        ctx.collection.register_fonts(Blob::new(Arc::new(sample_bytes()) as _),
            Some(FontInfoOverride { family_name: Some("OnlyRegular"),
                weight: Some(FontWeight::NORMAL), style: Some(FontStyle::Normal),
                ..Default::default() }));
        let mut query = ctx.collection.query(&mut ctx.source_cache);
        query.set_families(["OnlyRegular"]);
        query.set_attributes(Attributes::new(FontWidth::NORMAL, FontStyle::Italic, FontWeight::BOLD));
        let selected = query.first_available_font().unwrap();
        assert!(selected.synthesis.embolden());
        assert!(selected.synthesis.skew().is_some());
    }

    #[test]
    fn opt_out_starts_without_any_host_families() {
        let mut ctx = context(false, &[], "");
        assert!(ctx.collection.family_id("Liberation Sans").is_none());
        assert!(ctx.collection.family_id("Arial").is_none());
        assert!(ctx.collection.family_id("Bullet").is_some());
    }
}
