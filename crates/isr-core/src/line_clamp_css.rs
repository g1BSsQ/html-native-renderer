// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
//
// Keep the clamp declarations in the cascade even on builds whose style
// property registry does not yet expose these properties. Unknown declarations
// are otherwise discarded before layout can inspect them.
pub(crate) fn normalize(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = String::with_capacity(input.len());
    let mut emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b':' || i < emitted {
            i += 1;
            continue;
        }
        let mut end = i;
        while end > emitted && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > emitted
            && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'-')
        {
            start -= 1;
        }
        let property = &input[start..end];
        let mapped = match property.to_ascii_lowercase().as_str() {
            "line-clamp" => Some("--native-lines"),
            "-webkit-line-clamp" => Some("--native-legacy-lines"),
            "-webkit-box-orient" => Some("--native-legacy-orient"),
            "max-lines" => Some("--native-max-lines"),
            "block-ellipsis" => Some("--native-ellipsis"),
            _ => None,
        };
        if let Some(mapped) = mapped {
            result.push_str(&input[emitted..start]);
            result.push_str(mapped);
            result.push_str(&input[end..=i]);
            emitted = i + 1;
        } else if property.eq_ignore_ascii_case("display") {
            let mut value = i + 1;
            while value < bytes.len() && bytes[value].is_ascii_whitespace() {
                value += 1;
            }
            let legacy = ["-webkit-inline-box", "-webkit-box"]
                .into_iter()
                .find(|v| input[value..].starts_with(v)
                    && input.as_bytes().get(value + v.len()).is_none_or(|next|
                        !next.is_ascii_alphanumeric() && *next != b'-'));
            if let Some(legacy) = legacy {
                result.push_str(&input[emitted..start]);
                if legacy == "-webkit-inline-box" {
                    result.push_str("display:inline-block;--native-legacy-display:yes");
                } else {
                    result.push_str("display:flow-root;--native-legacy-display:yes");
                }
                emitted = value + legacy.len();
                i = emitted;
            }
        }
        i += 1;
    }
    result.push_str(&input[emitted..]);
    result
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn keeps_clamp_declarations_and_legacy_recipe() {
        let css = "display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; overflow: hidden; line-clamp: 3; max-lines: 4; block-ellipsis: none";
        let out = normalize(css);
        for part in ["display:flow-root", "--native-legacy-display:yes",
            "--native-legacy-orient: vertical", "--native-legacy-lines: 2",
            "--native-lines: 3", "--native-max-lines: 4", "--native-ellipsis: none"] {
            assert!(out.contains(part), "{part}: {out}");
        }
        assert!(out.contains("overflow: hidden"));
        assert_eq!(normalize("line-clamp: none"), "--native-lines: none");
    }
}
