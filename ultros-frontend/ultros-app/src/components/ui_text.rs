use colorsys::{ColorTransform, Rgb};
use leptos::*;

#[derive(Debug, PartialEq)]
enum Tag {
    ForegroundColor,
    BackgroundColor,
    Emphasis,
}

#[derive(Debug, Clone, Default)]
struct TextSpan<'a> {
    text: &'a str,
    foreground_color: Option<Rgb>,
    glow_color: Option<Rgb>,
    emphasis: bool,
}

#[derive(Debug, PartialEq, PartialOrd)]
struct TagData<'a> {
    tag_name: &'a str,
    tag_content: &'a str,
}

impl<'a> TagData<'a> {
    fn find_tag(text: &'a str) -> Option<(&'a str, Self, &'a str)> {
        let tag_start = text.find("<")?;
        let tag_end = text.find(">")?;
        let tag_name = &text[tag_start + 1..tag_end];
        let closing_tag_str = format!("</{tag_name}>");
        let closing_tag = text.find(&closing_tag_str)?;
        Some((
            &text[..tag_start],
            TagData {
                tag_name,
                tag_content: &text[&tag_end + 1..closing_tag],
            },
            &text[&closing_tag + closing_tag_str.len()..],
        ))
    }
}

impl<'a> TextSpan<'a> {
    fn new(text: &'a str) -> Option<(&'a str, Self, &'a str)> {
        // find a tag
        let (previous_part, tag, rest) = TagData::find_tag(text)?;
        let mut span = TextSpan::default();
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
                _ => clone.glow_color = Some(Rgb::from_hex_str(tag_content).unwrap()),
            },
            "UIForeground" => match tag_content {
                "01" => clone.foreground_color = None,
                _ => clone.foreground_color = Some(Rgb::from_hex_str(tag_content).unwrap()),
            },
            _ => panic!("Unknown item description tag: {tag_name}"),
        }
        clone
    }

    fn to_view(&self, cx: Scope) -> Option<View> {
        let Self {
            text,
            foreground_color,
            glow_color,
            emphasis,
        } = self;
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
        Some(view! {cx, <span style=styles.join(";")><BreakOnNewLine text/></span>}.into_view(cx))
    }
}

#[component]
fn BreakOnNewLine(cx: Scope, text: String) -> impl IntoView {
    let lines = text.split("\n");
    let mut views = vec![];
    let line_break = view! {cx, <br/> }.into_view(cx);
    for line in lines {
        views.push(view! {cx, {line.to_string()}}.into_view(cx));
        views.push(line_break.clone());
    }
    // let _ = views.pop();
    views
}

/// A UI component that takes the raw FFXIV text and converts it into HTML
/// For example: "This is unstyled <UIGlow>32113</UIGlow>blah blah<Emphasis>Hello world</Emphasis><UIGlow>01</UIGlow>" -> "This is unstyled <span style="color: #32113"><i>blah blah</i></span>"
#[component]
pub fn UIText(cx: Scope, text: String) -> impl IntoView {
    let mut text_parts = vec![];
    if let Some((begin, span, end)) = TextSpan::new(&text) {
        if !begin.is_empty() {
            text_parts.push(view! {cx, <BreakOnNewLine text=begin.to_owned()/>}.into_view(cx));
        }
        if let Some(view) = span.to_view(cx) {
            text_parts.push(view);
        }
        // now continue calling next_span until we reach the end of the rainbow
        let mut rest = end;
        let mut next_span = span;
        loop {
            let span = next_span.next_span(rest);
            match span {
                Ok((o, span, end)) => {
                    if let Some(o) = o {
                        if let Some(view) = o.to_view(cx) {
                            text_parts.push(view);
                        }
                    }
                    if let Some(o) = span.to_view(cx) {
                        text_parts.push(o);
                    }
                    rest = end;
                    next_span = span;
                }
                Err(view) => {
                    if let Some(view) = view.to_view(cx) {
                        text_parts.push(view);
                    }
                    break;
                }
            }
            if rest.is_empty() {
                break;
            }
        }
    } else {
        text_parts.push(view! {cx, <BreakOnNewLine text=text/>}.into_view(cx))
    }
    view! {cx, <div class="ui-text">{text_parts}</div>}
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

    // use super::UiTextElement;

    // #[test]
    // fn emphasis() {
    //     let ui_text_element =
    //         "copies of <Emphasis>Tales of Adventure: One Dragoon's Journey III</Emphasis>";
    //     assert_eq!(
    //         UiTextElement::new(ui_text_element),
    //         UiTextElement::Elements(vec![
    //             UiTextElement::Text("copies of "),
    //             UiTextElement::Emphasis(Box::new(UiTextElement::Text(
    //                 "Tales of Adventure: One Dragoon's Journey III"
    //             )))
    //         ])
    //     );
    // }
}
