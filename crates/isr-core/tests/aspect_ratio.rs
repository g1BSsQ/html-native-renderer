// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use isr_core::{render, RenderOptions};

fn assert_box(html: &str, id: &str, width: f64, height: f64) {
    let output = render(html, RenderOptions {
        width: 500, ..RenderOptions::default()
    }).unwrap();
    assert!(output.blocked_resources.is_empty());
    let el = output.elements.iter().find(|el| el.id == id).unwrap();
    assert!((el.width - width).abs() < 0.15, "{id}: width {} != {width}", el.width);
    assert!((el.height - height).abs() < 0.15, "{id}: height {} != {height}", el.height);
}

// The automatic minimum must be allowed to exceed the preferred aspect-ratio
// size, after converting between content and border boxes.
#[test]
fn block_content_box_ratio_respects_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{width:60px;aspect-ratio:2/1;padding:10px;border:10px solid green}
        </style><div id=box><div style="height:60px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

#[test]
fn block_border_box_ratio_respects_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{box-sizing:border-box;width:100px;aspect-ratio:2/1;
        padding:10px;border:10px solid green}
        </style><div id=box><div style="height:60px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

// A transferred maximum cannot suppress the content-based automatic minimum.
#[test]
fn max_width_transfer_does_not_cap_content_automatic_minimum() {
    let html = r#"<style>body{margin:0}
        #box{width:200px;max-width:100px;aspect-ratio:2/1}
        </style><div id=box><div style="height:100px"></div></div>"#;
    assert_box(html, "box", 100.0, 100.0);
}

const NATURAL_RATIO_IMAGE: &str =
    "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='80'%20height='40'%3E%3C/svg%3E";

#[test]
fn replaced_auto_ratio_prefers_natural_ratio() {
    let html = format!(r#"<style>body{{margin:0}}#image{{width:120px;aspect-ratio:auto 1/1}}</style>
        <img id=image src="{NATURAL_RATIO_IMAGE}">"#);
    assert_box(&html, "image", 120.0, 60.0);
}

// A flex item's content height follows the replaced content's natural
// ratio. The outer item stays in the flex formatting context.
#[test]
fn flex_item_content_contribution_uses_natural_auto_ratio() {
    let html = format!(r#"<style>body{{margin:0}}
        #row{{display:flex;align-items:flex-start}}
        #item{{width:120px;flex:none}}
        #image{{display:block;width:120px;aspect-ratio:auto 1/1}}
        </style><div id=row><div id=item>
          <img id=image src="{NATURAL_RATIO_IMAGE}">
        </div></div>"#);
    assert_box(&html, "image", 120.0, 60.0);
    assert_box(&html, "item", 120.0, 60.0);
}

#[test]
fn grid_auto_track_contribution_uses_natural_auto_ratio() {
    let html = format!(r#"<style>body{{margin:0}}
        #grid{{display:grid;grid-template-columns:120px}}
        #item{{width:120px;align-self:start}}
        #image{{display:block;width:120px;aspect-ratio:auto 1/1}}
        </style><div id=grid><div id=item>
          <img id=image src="{NATURAL_RATIO_IMAGE}">
        </div></div>"#);
    assert_box(&html, "image", 120.0, 60.0);
    assert_box(&html, "item", 120.0, 60.0);
}

// Guard the already-correct margin-collapsing behavior. The positive bottom
// margin of a zero-height child is not its parent’s content minimum size.
#[test]
fn collapsed_child_margins_do_not_enlarge_ratio_sized_parent() {
    let html = r#"<style>body{margin:0}</style>
        <div id=box style="width:100px;aspect-ratio:2/1;margin:0">
          <div style="width:100px;margin-top:50px;margin-bottom:200px"></div>
        </div>
        <div style="width:100px;height:25px"></div>"#;
    assert_box(html, "box", 100.0, 50.0);
}

// Regression guard: stretch in the flex cross axis should determine the
// ratio-dependent main size even with no preferred width.
#[test]
fn flex_stretched_cross_size_transfers_preferred_ratio() {
    let html = r#"<style>body{margin:0}</style>
        <div style="display:flex;height:50px">
          <div id=item style="aspect-ratio:2/1;flex:none"></div>
        </div>"#;
    assert_box(html, "item", 100.0, 50.0);
}

// The unvendored grid layout does not currently transfer a definite row
// height into the auto-width item's preferred ratio.
#[test]
#[ignore = "Needs ratio-dependent width transfer in unvendored grid layout"]
fn grid_definite_row_transfers_preferred_ratio() {
    let html = r#"<style>body{margin:0}</style>
        <div style="display:grid;grid-template-rows:50px">
          <div id=item style="aspect-ratio:2/1;justify-self:start"></div>
        </div>"#;
    assert_box(html, "item", 100.0, 50.0);
}

// The scroll-container exception does not apply to non-scrollable overflow:clip.
#[test]
fn block_auto_minimum_respects_scrollable_overflow_exception() {
    for (overflow, height) in [("hidden", 50.0), ("scroll", 50.0), ("clip", 120.0)] {
        let html = format!(r#"<style>body{{margin:0}}</style>
          <div id=box style="width:100px;aspect-ratio:2/1;overflow:{overflow}">
            <div style="height:120px"></div>
          </div>"#);
        assert_box(&html, "box", 100.0, height);
    }
}

// Inline-root line boxes, not only atomic/block children, contribute to
// the automatic content-based minimum of a ratio-sized box.
#[test]
fn inline_root_three_lines_protect_automatic_height() {
    let html = r#"<style>body{margin:0}</style>
      <div id=natural style="width:100px;
          font-size:16px;line-height:30px">one<br>two<br>three</div>
      <div id=box style="width:100px;aspect-ratio:2/1;
          font-size:16px;line-height:30px">one<br>two<br>three</div>"#;
    let output = render(html, RenderOptions {
        width:500, system_fonts:true, ..RenderOptions::default()
    }).unwrap();
    let natural = output.elements.iter().find(|el| el.id == "natural").unwrap();
    let box_ = output.elements.iter().find(|el| el.id == "box").unwrap();
    assert!(natural.height > 50.0, "natural three-line height {} <= 50", natural.height);
    assert!((box_.height - natural.height).abs() < 0.15,
        "ratio box height {} != three-line content height {}", box_.height, natural.height);
}

// Regression guard: a definite height imposed by a flex parent outranks
// automatic-minimum growth. This was already correct on the earlier head.
#[test]
fn definite_parent_height_is_not_grown_by_ratio_minimum() {
    let html = r#"<style>body{margin:0}</style>
      <div style="display:flex;height:50px">
        <div id=item style="aspect-ratio:1">
          <div style="height:100px"></div>
        </div>
      </div>"#;
    let output = render(html, RenderOptions { width:500, ..RenderOptions::default() }).unwrap();
    let el = output.elements.iter().find(|el| el.id == "item").unwrap();
    assert!((el.height - 50.0).abs() < 0.15, "item: height {} != 50", el.height);
}

// Bottom margins of in-flow children contribute when the parent's bottom
// padding prevents them from collapsing through its border edge.
#[test]
fn bottom_padding_keeps_child_bottom_margin_in_content_minimum() {
    let html = r#"<style>body{margin:0}</style>
        <div id=box style="width:100px;aspect-ratio:2/1;padding:1px">
          <div style="height:60px;margin-bottom:30px"></div>
        </div>"#;
    assert_box(html, "box", 102.0, 92.0);
}

// Regression guard: a floated shrink-to-fit width retains the ratio-derived
// floor for an auto height. This was already correct before the review.
// The actual text advance depends on the font; the square relationship does not.
#[test]
fn shrink_to_fit_float_retains_ratio_height_floor() {
    let html = r#"<style>body{margin:0}</style>
        <div id=box style="float:left;aspect-ratio:1;
           white-space:nowrap;font-size:16px;line-height:16px">Sufficiently long inline text</div>"#;
    let output = render(html, RenderOptions { width:500, ..RenderOptions::default() }).unwrap();
    let el = output.elements.iter().find(|el| el.id == "box").unwrap();
    assert!(el.width > 30.0, "shrink-to-fit width {} too small", el.width);
    assert!((el.height - el.width).abs() < 0.15,
        "shrink-to-fit ratio lost: width {} height {}", el.width, el.height);
}
