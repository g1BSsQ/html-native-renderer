// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
// System fontconfig access is gated by RenderOptions::system_fonts on Linux.
use blitz_dom::FontContext;
use parley::fontique::{
    Blob, Collection, CollectionOptions, GenericFamily, SourceCache,
};
use std::sync::Arc;

pub(crate) fn context(system_fonts: bool, supplied: &[Vec<u8>], html: &str) -> FontContext {
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions { shared: false, system_fonts }),
    };
    // The marker font retains its own family, Bullet, regardless of host fonts.
    ctx.collection.register_fonts(Blob::new(Arc::new(blitz_dom::BULLET_FONT) as _), None);
    for bytes in supplied {
        let decoded = blitz_dom::decode_font_bytes(bytes).into_owned();
        let registered = ctx.collection.register_fonts(Blob::new(Arc::new(decoded) as _), None);
        // Keep the pre-existing host-supplied-font generic fallback behaviour.
        let ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
        for generic in [
            GenericFamily::SansSerif, GenericFamily::Serif,
            GenericFamily::Monospace, GenericFamily::SystemUi,
        ] {
            ctx.collection.append_generic_families(generic, ids.iter().copied());
        }
    }
    if system_fonts {
        #[cfg(target_os = "linux")]
        fontconfig::configure(&mut ctx.collection, html);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = html;
    ctx
}

#[cfg(target_os = "linux")]
mod fontconfig {
    use super::*;
    use fontconfig_sys as fc;
    use parley::fontique::FontInfoOverride;
    use std::collections::{BTreeSet, HashMap};
    use std::ffi::{CStr, CString};
    use std::fs;
    use std::path::PathBuf;
    use std::ptr;
    use std::sync::{Mutex, OnceLock};

    // These are preferences from the Chromium-on-Ubuntu CI reference boxes,
    // not an assumed platform-wide Chromium mapping. The installed family must
    // actually exist; if absent, Fontique's normal fallback remains in place.
    const GENERICS: [(GenericFamily, &str); 6] = [
        (GenericFamily::Serif, "Liberation Serif"),
        (GenericFamily::SansSerif, "Liberation Sans"),
        (GenericFamily::Monospace, "Liberation Mono"),
        (GenericFamily::SystemUi, "Liberation Sans"),
        (GenericFamily::Cursive, "Liberation Serif"),
        (GenericFamily::Fantasy, "Liberation Serif"),
    ];

    // Explicit Chromium reference compatibility families. Do not turn an
    // arbitrary weak fontconfig match into an alias: an unknown first family
    // must allow the next CSS family to be tried.
    const ALIASES: [(&str, &str); 7] = [
        ("Arial", "Liberation Sans"),
        ("Helvetica", "Liberation Sans"),
        ("Georgia", "Liberation Serif"),
        ("Times New Roman", "Liberation Serif"),
        ("Times", "Liberation Serif"),
        ("Courier New", "Liberation Mono"),
        ("Courier", "Liberation Mono"),
    ];

    struct Cache {
        config: *mut fc::FcConfig,
        paths: HashMap<String, Vec<PathBuf>>,
        bytes: HashMap<PathBuf, Arc<Vec<u8>>>,
    }
    // All access to the native configuration and cache is behind its Mutex.
    unsafe impl Send for Cache {}

    impl Drop for Cache {
        fn drop(&mut self) {
            // SAFETY: config is owned by this Cache, used only while locked.
            unsafe { fc::FcConfigDestroy(self.config); }
        }
    }

    static CACHE: OnceLock<Option<Mutex<Cache>>> = OnceLock::new();

    fn cache() -> Option<&'static Mutex<Cache>> {
        CACHE.get_or_init(|| {
            // Only reached through configure() with explicit opt-in.
            // SAFETY: libfontconfig returns an owned config pointer.
            let config = unsafe { fc::FcInitLoadConfigAndFonts() };
            if config.is_null() {
                None
            } else {
                Some(Mutex::new(Cache {
                    config, paths: HashMap::new(), bytes: HashMap::new(),
                }))
            }
        }).as_ref()
    }

    // A family substring in unrelated prose ("sometimes") is not a request.
    fn requested(html: &str, family: &str) -> bool {
        let hay = html.as_bytes();
        let needle = family.as_bytes();
        if needle.is_empty() { return false; }
        hay.windows(needle.len()).enumerate().any(|(i, part)| {
            part.eq_ignore_ascii_case(needle)
                && (i == 0 || !name_byte(hay[i - 1]))
                && (i + needle.len() == hay.len() || !name_byte(hay[i + needle.len()]))
        })
    }

    fn name_byte(ch: u8) -> bool {
        ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_'
    }

    unsafe fn pattern_string(pattern: *mut fc::FcPattern, key: &CStr, index: i32) -> Option<String> {
        let mut value: *mut fc::FcChar8 = ptr::null_mut();
        // SAFETY: caller holds the fontconfig Mutex and owns pattern.
        if unsafe { fc::FcPatternGetString(pattern, key.as_ptr(), index, &mut value) } != fc::FcResultMatch || value.is_null() {
            return None;
        }
        // SAFETY: fontconfig owns NUL-terminated value until pattern is freed.
        Some(unsafe { CStr::from_ptr(value.cast()) }.to_string_lossy().into_owned())
    }

    impl Cache {
        fn matched_family(&mut self, name: &str) -> Option<String> {
            let name = CString::new(name).ok()?;
            // SAFETY: all pointers are owned for the duration of this call;
            // the process-wide Cache Mutex serializes access to config.
            unsafe {
                let pattern = fc::FcPatternCreate();
                if pattern.is_null() { return None; }
                fc::FcPatternAddString(pattern, fc::constants::FC_FAMILY.as_ptr(), name.as_ptr().cast());
                fc::FcConfigSubstitute(self.config, pattern, fc::FcMatchPattern);
                fc::FcDefaultSubstitute(pattern);
                let mut result = fc::FcResultMatch;
                let selected = fc::FcFontMatch(self.config, pattern, &mut result);
                let name = if selected.is_null() {
                    None
                } else {
                    pattern_string(selected, fc::constants::FC_FAMILY, 0)
                };
                if !selected.is_null() { fc::FcPatternDestroy(selected); }
                fc::FcPatternDestroy(pattern);
                name
            }
        }

        fn family_paths(&mut self, family: &str) -> Vec<PathBuf> {
            let key = family.to_ascii_lowercase();
            if let Some(paths) = self.paths.get(&key) { return paths.clone(); }
            let mut paths = BTreeSet::new();
            if let Ok(name) = CString::new(family) {
                // SAFETY: FcFontList is a cache-backed metadata query for one
                // installed family. It does not parse every font file.
                unsafe {
                    let pattern = fc::FcPatternCreate();
                    let objects = fc::FcObjectSetCreate();
                    if !pattern.is_null() && !objects.is_null() {
                        fc::FcPatternAddString(pattern, fc::constants::FC_FAMILY.as_ptr(), name.as_ptr().cast());
                        fc::FcObjectSetAdd(objects, fc::constants::FC_FAMILY.as_ptr());
                        fc::FcObjectSetAdd(objects, fc::constants::FC_FILE.as_ptr());
                        let matches = fc::FcFontList(self.config, pattern, objects);
                        if !matches.is_null() {
                            for i in 0..(*matches).nfont.max(0) as usize {
                                let face = *(*matches).fonts.add(i);
                                let matched = pattern_string(face, fc::constants::FC_FAMILY, 0);
                                if matched.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(family)) {
                                    if let Some(file) = pattern_string(face, fc::constants::FC_FILE, 0) {
                                        let path = PathBuf::from(file);
                                        if path.is_absolute() { paths.insert(path); }
                                    }
                                }
                            }
                            fc::FcFontSetDestroy(matches);
                        }
                    }
                    if !objects.is_null() { fc::FcObjectSetDestroy(objects); }
                    if !pattern.is_null() { fc::FcPatternDestroy(pattern); }
                }
            }
            let paths = paths.into_iter().collect::<Vec<_>>();
            self.paths.insert(key, paths.clone());
            paths
        }

        fn family_blobs(&mut self, family: &str) -> Vec<Arc<Vec<u8>>> {
            self.family_paths(family).into_iter().filter_map(|path| {
                if let Some(bytes) = self.bytes.get(&path) { return Some(bytes.clone()); }
                let bytes = Arc::new(fs::read(&path).ok()?);
                self.bytes.insert(path, bytes.clone());
                Some(bytes)
            }).collect()
        }
    }

    pub(super) fn configure(collection: &mut Collection, html: &str) {
        let Some(cache) = cache() else { return };
        let Ok(mut cache) = cache.lock() else { return };

        for (alias, target) in ALIASES {
            if !requested(html, alias) || collection.family_id(alias).is_some() { continue; }
            // Match in-process, through the host's configured fontconfig. A
            // curated CI-compatible target is allowed only if installed and
            // selected, or if the CI's measured compatibility target is present.
            let matched = cache.matched_family(alias);
            let selected = if matched.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(target)) {
                target
            } else {
                // CI-observed Georgia compatibility is preferable to an unrelated
                // weak default. Never accept an arbitrary unrelated match.
                if alias != "Georgia" { continue; }
                target
            };
            let faces = cache.family_blobs(selected);
            if faces.is_empty() { continue; }
            for bytes in faces {
                collection.register_fonts(Blob::new(bytes as _),
                    Some(FontInfoOverride { family_name: Some(alias), ..Default::default() }));
            }
        }
        // Prepend the CI reference preference without deleting the backend's
        // ordered per-character fallback list.
        for (generic, first) in GENERICS {
            let Some(id) = collection.family_id(first) else { continue };
            let existing: Vec<_> = collection.generic_families(generic).collect();
            collection.set_generic_families(generic,
                std::iter::once(id).chain(existing.into_iter().filter(|x| *x != id)));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_fontconfig_resolves_compatibility_families() {
            let mut c = context(true, &[], "<p style='font-family:Helvetica, Times, Courier'>demo</p>");
            for alias in ["Helvetica", "Times", "Courier"] {
                assert!(c.collection.family_by_name(alias).is_some(), "{alias} must resolve on the CI image");
            }
            let cache = cache().unwrap().lock().unwrap();
            assert_eq!(cache.matched_family("Arial").as_deref(), Some("Liberation Sans"));
        }
        #[test]
        fn substring_does_not_trigger_alias_lookup() {
            assert!(!requested("sometimes", "Times"));
            assert!(requested("font-family:Times,serif", "Times"));
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
