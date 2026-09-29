use colorsys::{ColorTransform, Rgb};
use leptos::{either::Either, html::br, prelude::*};

#[derive(Debug, Clone, Default)]
struct TextSpan<'a> {
    text: &'a str,
    foreground_color: Option<Box<Rgb>>,
    glow_color: Option<Box<Rgb>>,
    emphasis: bool,
}

#[derive(Debug, PartialEq, PartialOrd)]
struct TagData<'a> {
    tag_name: &'a str,
    tag_content: &'a str,
}

impl<'a> TagData<'a> {
    fn find_tag(text: &'a str) -> Option<(&'a str, Self, &'a str)> {
        let tag_start = text.find('<')?;
        let tag_end = text.find('>')?;
        let tag_name = &text[tag_start + 1..tag_end];

        // Zero-allocation search for the closing tag to prevent creating a String per parsing step
        let mut search_start = tag_end + 1;
        let mut text_rem = &text[search_start..];
        let (closing_tag, closing_tag_end) = loop {
            let idx = text_rem.find("</")?;
            let abs_idx = search_start + idx;
            let after_open = &text[abs_idx + 2..];
            if after_open.starts_with(tag_name) && after_open[tag_name.len()..].starts_with('>') {
                break (abs_idx, abs_idx + 2 + tag_name.len() + 1);
            }
            search_start = abs_idx + 2;
            text_rem = &text[search_start..];
        };

        Some((
            &text[..tag_start],
            TagData {
                tag_name,
                tag_content: &text[&tag_end + 1..closing_tag],
            },
            &text[closing_tag_end..],
        ))
    }
}

impl<'a> TextSpan<'a> {
    fn new(text: &'a str) -> Option<(&'a str, Self, &'a str)> {
        // find a tag
        let (previous_part, tag, rest) = TagData::find_tag(text)?;
        let span = TextSpan::default();
        let span = span.read_tag_data(tag);
        Some((previous_part, span, rest))
    }

    fn next_span(&self, rest: &'a str) -> Result<(Option<Self>, Self, &'a str), Self> {
        let (previous_part, tag, rest) = TagData::find_tag(rest).ok_or_else(|| {
            let mut data = self.clone();
            data.emphasis = false;
            data.text = rest;
            data
        })?;
        let previous_part = if !previous_part.is_empty() {
            let mut previous_span = self.clone();
            previous_span.text = previous_part;
            Some(previous_span)
        } else {
            None
        };
        let span = self.read_tag_data(tag);
        Ok((previous_part, span, rest))
    }

    fn read_tag_data(&self, tag_data: TagData<'a>) -> Self {
        let TagData {
            tag_name,
            tag_content,
        } = tag_data;
        let mut clone = self.clone();
        clone.text = "";
        clone.emphasis = false;
        match tag_name {
            "Emphasis" => {
                clone.emphasis = true;
                clone.text = tag_content;
            }
            "UIGlow" => match tag_content {
                "01" => clone.glow_color = None,
                _ => clone.glow_color = Some(Box::new(Rgb::from_hex_str(tag_content).unwrap())),
            },
            "UIForeground" => match tag_content {
                "01" => clone.foreground_color = None,
                _ => {
                    clone.foreground_color = Some(Box::new(Rgb::from_hex_str(tag_content).unwrap()))
                }
            },
            _ => panic!("Unknown item description tag: {tag_name}"),
        }
        clone
    }

    fn to_view(&self) -> Option<impl IntoView + use<>> {
        let Self { text, .. } = self;
        if text.is_empty() {
            return None;
        }

        let styles = [
            self.foreground_color.clone().map(|mut color| {
                color.invert();
                let color = color.to_css_string();
                format!("color: {color}")
            }),
            self.glow_color.clone().map(|mut glow_color| {
                glow_color.invert();
                let glow_color = glow_color.to_css_string();
                format!("text-shadow:1px 1px 2px #{glow_color}, 1px 1px 2px #{glow_color}")
            }),
            self.emphasis.then(|| "font-style: italic".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<String>>();
        let text = text.to_string();
        let span = view! {
            <div style=styles.join(";")>
                <RawText text />
            </div>
        };
        Some(span)
    }
}

#[component]
fn RawText<'a>(#[prop(into)] text: Oco<'a, str>) -> impl IntoView {
    let mut text_parts = vec![];
    for line in text.lines() {
        text_parts.push(Either::Left(line.to_owned().into_view()));
        text_parts.push(Either::Right(br()));
    }
    text_parts.pop();
    text_parts
}

#[component]
fn TextParts(text: String) -> impl IntoView {
    let mut text_parts = vec![];
    if let Some((begin, span, end)) = TextSpan::new(text.as_str()) {
        if !begin.is_empty() {
            text_parts.push(Either::Left(view! { <RawText text=begin.to_owned() /> }));
        }
        if let Some(view) = span.to_view() {
            text_parts.push(Either::Right(view));
        }
        // now continue calling next_span until we reach the end of the rainbow
        let mut rest = end;
        let mut next_span = span;
        loop {
            let span = next_span.next_span(rest);
            match span {
                Ok((o, span, end)) => {
                    if let Some(o) = o
                        && let Some(view) = o.to_view()
                    {
                        text_parts.push(Either::Right(view));
                    }
                    if let Some(o) = span.to_view() {
                        text_parts.push(Either::Right(o));
                    }
                    rest = end;
                    next_span = span;
                }
                Err(view) => {
                    if let Some(view) = view.to_view() {
                        text_parts.push(Either::Right(view));
                    }
                    break;
                }
            }
            if rest.is_empty() {
                break;
            }
        }
    } else {
        text_parts.push(Either::Left(view! { <RawText text /> }));
    }
    text_parts.into_any()
}

/// A UI component that takes the raw FFXIV text and converts it into HTML
/// For example: "This is unstyled <UIGlow>32113</UIGlow>blah blah<Emphasis>Hello world</Emphasis><UIGlow>01</UIGlow>" -> "This is unstyled <span class="text-brand-300 italic">blah blah</span>"
#[component]
pub fn UIText(text: String) -> impl IntoView {
    view! {
        <div class="ui-text">
            <TextParts text=text />
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::TagData;

    #[test]
    fn find_tags() {
        let test_string =
            "blah blah hello world <UiColor>01</UiColor> test 123 <Emphasis>Hello world</Emphasis>";
        assert_eq!(
            TagData::find_tag(test_string),
            Some((
                "blah blah hello world ",
                TagData {
                    tag_name: "UiColor",
                    tag_content: "01"
                },
                " test 123 <Emphasis>Hello world</Emphasis>"
            ))
        );
        let rest_string = " test 123 <Emphasis>Hello world</Emphasis>";
        assert_eq!(
            TagData::find_tag(rest_string),
            Some((
                " test 123 ",
                TagData {
                    tag_name: "Emphasis",
                    tag_content: "Hello world"
                },
                ""
            ))
        );
    }

    use super::TextSpan;

    #[test]
    fn text_span_state_machine() {
        let text = "Unstyled <UIGlow>F8F8F8</UIGlow>Glowing text<UIGlow>01</UIGlow> Normal <Emphasis>Italic</Emphasis>";
        let (prev, span, rest) = TextSpan::new(text).unwrap();
        assert_eq!(prev, "Unstyled ");
        assert_eq!(span.text, "");
        assert!(span.glow_color.is_some());

        let (prev2_opt, span2, rest2) = span.next_span(rest).unwrap();
        let prev2 = prev2_opt.unwrap();
        assert_eq!(prev2.text, "Glowing text");
        assert!(prev2.glow_color.is_some());

        assert!(span2.glow_color.is_none());
        assert_eq!(span2.text, "");

        let (prev3_opt, span3, rest3) = span2.next_span(rest2).unwrap();
        let prev3 = prev3_opt.unwrap();
        assert_eq!(prev3.text, " Normal ");
        assert!(prev3.glow_color.is_none());

        assert_eq!(span3.text, "Italic");
        assert!(span3.emphasis);
        assert_eq!(rest3, "");
    }
}
