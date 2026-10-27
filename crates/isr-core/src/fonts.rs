// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// Host fontconfig is consulted only when RenderOptions::system_fonts is true.
use blitz_dom::FontContext;
use parley::fontique::{
    Blob, Collection, CollectionOptions, FontInfoOverride, GenericFamily, SourceCache,
};
use std::sync::Arc;

pub(crate) fn context(system_fonts: bool, supplied: &[Vec<u8>]) -> FontContext {
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
        fontconfig::configure(&mut ctx.collection);
    }
    ctx
}

#[cfg(target_os = "linux")]
mod fontconfig {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const GENERICS: [(&str, GenericFamily); 6] = [
        ("serif", GenericFamily::Serif),
        ("sans-serif", GenericFamily::SansSerif),
        ("monospace", GenericFamily::Monospace),
        ("system-ui", GenericFamily::SystemUi),
        ("cursive", GenericFamily::Cursive),
        ("fantasy", GenericFamily::Fantasy),
    ];

    // These are *queries*, not a table of presumed substitutions. The matching
    // installed family is returned by the host's fontconfig configuration.
    const COMPATIBILITY_QUERIES: &[&str] = &[
        "Arial", "Helvetica", "Times New Roman", "Times", "Courier New",
        "Courier", "Georgia", "Verdana", "Trebuchet MS", "Tahoma", "Calibri",
        "Cambria", "Consolas", "Andale Mono", "Comic Sans MS", "Impact",
        "Palatino", "Bookman",
    ];

    fn command(tool: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(tool).args(args).output().ok()?;
        output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
    }

    fn first_family(s: &str) -> Option<&str> {
        s.trim().split(',').next().map(str::trim).filter(|s| !s.is_empty())
    }

    fn match_family(name: &str) -> Option<String> {
        let output = command("fc-match", &["-f", "%{family}\n", name])?;
        first_family(&output).map(str::to_owned)
    }

    // Parse fontconfig's installed-face listing, not guessed font filenames.
    fn installed_faces() -> BTreeMap<String, BTreeSet<PathBuf>> {
        let mut result = BTreeMap::<String, BTreeSet<PathBuf>>::new();
        if let Some(output) = command("fc-list", &["-f", "%{family}\t%{file}\n"]) {
            for line in output.lines() {
                let Some((names, path)) = line.split_once('\t') else { continue };
                let path = Path::new(path.trim());
                if !path.is_absolute() { continue; }
                for name in names.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    result.entry(name.to_ascii_lowercase()).or_default().insert(path.to_path_buf());
                }
            }
        }
        result
    }

    // Extract alias request names from fontconfig's installed XML. The selected
    // target is still resolved by fc-match so preferences and installed fonts
    // decide the actual face. Parsing stops before <prefer>/<accept>/<default>.
    fn alias_names_in(xml: &str, names: &mut BTreeSet<String>) {
        for segment in xml.split("<alias").skip(1) {
            let Some((body, _)) = segment.split_once("</alias>") else { continue };
            let body = body.split("<prefer>").next().unwrap_or(body)
                .split("<accept>").next().unwrap_or(body)
                .split("<default>").next().unwrap_or(body);
            let mut rest = body;
            while let Some((_, tail)) = rest.split_once("<family>") {
                let Some((family, next)) = tail.split_once("</family>") else { break };
                let family = family.trim();
                if !family.is_empty() { names.insert(family.to_owned()); }
                rest = next;
            }
        }
    }

    fn configured_alias_queries() -> BTreeSet<String> {
        let mut names: BTreeSet<String> = COMPATIBILITY_QUERIES.iter().map(|s| (*s).to_owned()).collect();
        // Only called behind the explicit system-fonts opt-in.
        for root in ["/etc/fonts/conf.d", "/etc/fonts/conf.avail"] {
            if let Ok(files) = fs::read_dir(root) {
                for file in files.flatten() {
                    if file.path().extension().and_then(|s| s.to_str()) != Some("conf") { continue; }
                    if let Ok(xml) = fs::read_to_string(file.path()) {
                        alias_names_in(&xml, &mut names);
                    }
                }
            }
        }
        if let Ok(xml) = fs::read_to_string("/etc/fonts/fonts.conf") {
            alias_names_in(&xml, &mut names);
        }
        names
    }

    pub(super) fn configure(collection: &mut Collection) {
        // The collection's installed families are supplied by its system backend.
        // Override generic ordering with the first fc-match family on this host.
        for (name, generic) in GENERICS {
            if let Some(selected) = match_family(name) {
                if let Some(id) = collection.family_id(&selected) {
                    collection.set_generic_families(generic, std::iter::once(id));
                }
            }
        }

        let installed = installed_faces();
        let mut cache = BTreeMap::<PathBuf, Arc<Vec<u8>>>::new();
        for alias in configured_alias_queries() {
            // A real installed family takes precedence over fontconfig aliases.
            if collection.family_id(&alias).is_some() { continue; }
            let Some(selected) = match_family(&alias) else { continue };
            if alias.eq_ignore_ascii_case(&selected) { continue; }
            let Some(paths) = installed.get(&selected.to_ascii_lowercase()) else { continue };
            for path in paths {
                let data = if let Some(data) = cache.get(path) {
                    data.clone()
                } else {
                    let Ok(bytes) = fs::read(path) else { continue };
                    let data = Arc::new(bytes);
                    cache.insert(path.clone(), data.clone());
                    data
                };
                collection.register_fonts(Blob::new(data as _), Some(FontInfoOverride {
                    family_name: Some(&alias), ..Default::default()
                }));
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn fontconfig_arial_maps_to_an_installed_sans_face() {
            let arial = match_family("Arial").expect("fontconfig must be installed for this Linux test");
            let sans = match_family("Liberation Sans").expect("fontconfig must resolve the installed CI sans face");
            assert_eq!(arial, sans, "Arial should select the installed metric-compatible sans face");
        }
        #[test]
        fn configured_xml_alias_requests_are_extracted() {
            let mut names = BTreeSet::new();
            alias_names_in("<alias binding=\"same\"><family>Example Sans</family><prefer><family>Other</family></prefer></alias>", &mut names);
            assert!(names.contains("Example Sans"));
            assert!(!names.contains("Other"));
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
        let mut ctx = context(false, &[]);
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
        let mut ctx = context(false, &[]);
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
        let mut ctx = context(false, &[]);
        assert!(ctx.collection.family_id("Liberation Sans").is_none());
        assert!(ctx.collection.family_id("Arial").is_none());
        assert!(ctx.collection.family_id("Bullet").is_some());
    }
}
