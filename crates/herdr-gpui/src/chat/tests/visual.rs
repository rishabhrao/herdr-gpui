use super::*;
use crate::{
    chat::{
        prepared::{Body, Part, Prepared},
        visual::{self, DiagramStyle, Error, Shown, Source, VisualId, Visuals},
    },
    config::{Config, Theme},
};
use base64::Engine as _;
use gpui::TestAppContext;
use herdr_chat_protocol::{Author, Image};
use std::{collections::HashSet, io::Cursor};

const FLOW: &str = "flowchart LR\n  A[Ask] --> B[Draw]\n";

fn png(width: u32, height: u32) -> Image {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Image::new("image/png", data).unwrap()
}

pub(super) fn message(text: &str) -> (Prepared, Vec<String>) {
    let mut transcript = Transcript::new("Claude");
    transcript.entries = vec![agent(1, text)];
    let mut prepared = Prepared::default();
    prepared.update(&transcript, &[(EntryId(1), 1)]);
    let Body::Message(parts) = &*prepared.body(EntryId(1)).unwrap() else {
        panic!("not a message");
    };
    let shape = parts
        .iter()
        .map(|part| match part {
            Part::Text(lines) => format!("text:{}", lines[0].text),
            Part::Diagram(..) => "diagram".to_owned(),
            Part::Html { .. } => "page".to_owned(),
            Part::Picture { alt } => format!("picture:{alt}"),
            Part::Remote { url, .. } => format!("remote:{}", url.as_str()),
        })
        .collect();
    (prepared, shape)
}

fn styled(background: u32) -> DiagramStyle {
    let theme = Theme {
        background,
        ..Theme::default()
    };
    DiagramStyle::of(&theme, &Config::default().ui)
}

#[test]
fn a_closed_mermaid_fence_becomes_a_diagram_between_text() {
    let (prepared, shape) = message(&format!("Here:\n```mermaid\n{FLOW}```\nDone."));
    assert_eq!(shape, ["text:Here:", "diagram", "text:Done."]);
    let id = VisualId::of(FLOW);
    assert!(matches!(prepared.visuals.shown(id), Shown::Pending));
    assert_eq!(prepared.drawing(id).collect::<Vec<_>>(), [EntryId(1)]);
}

#[test]
fn a_fence_still_streaming_or_too_long_stays_code() {
    let (_, shape) = message(&format!("```mermaid\n{FLOW}"));
    assert_eq!(shape, ["text:flowchart LR"]);
    let long = format!(
        "```mermaid\n{}\n```\n",
        "A-->B\n".repeat(visual::MAX_DIAGRAM_SOURCE)
    );
    assert_eq!(message(&long).1, ["text:A-->B"]);
    // Other fences are code as before.
    assert_eq!(
        message("```rust\nfn main() {}\n```\n").1,
        ["text:fn main() {}"]
    );
}

#[test]
fn image_entries_and_tool_images_ask_for_visuals() {
    let image = png(2, 2);
    let mut transcript = Transcript::new("Claude");
    transcript.entries = vec![Entry {
        id: EntryId(1),
        kind: EntryKind::Image {
            author: Author::Agent,
            image: image.clone(),
        },
    }];
    let mut prepared = Prepared::default();
    prepared.update(&transcript, &[(EntryId(1), 1)]);
    let id = VisualId::of(&image.data);
    assert!(matches!(&*prepared.body(EntryId(1)).unwrap(), Body::Image(found) if *found == id));
    transcript.entries.clear();
    prepared.update(&transcript, &[]);
    assert!(prepared.drawing(id).next().is_none());
}

#[test]
fn an_image_decodes_at_its_pixel_size() {
    let drawn = visual::decode(&png(3, 2)).unwrap();
    assert_eq!((drawn.width, drawn.height), (3., 2.));
    let pixels = drawn.texture.as_bytes(0).unwrap();
    // GPUI takes BGRA.
    assert_eq!(&pixels[..4], &[30, 30, 200, 255]);
    let bad = Image::new("image/png", "AAAA".to_owned()).unwrap();
    assert!(matches!(visual::decode(&bad), Err(Error::Decode(_))));
    let odd = Image::new("image/png", "A".to_owned()).unwrap();
    assert!(matches!(visual::decode(&odd), Err(Error::Base64(_))));
}

#[gpui::test]
fn diagrams_are_drawn_off_thread_and_redrawn_for_a_new_theme(cx: &mut TestAppContext) {
    let renderer = cx.update(|cx| cx.svg_renderer());
    let mut visuals = Visuals::default();
    let id = VisualId::of(FLOW);
    visuals.want(id, Source::Diagram(FLOW.into()));
    let sequence = "sequenceDiagram\n  participant U as You\n  U->>A: prompt\n  A-->>U: answer\n";
    let other = VisualId::of(sequence);
    visuals.want(other, Source::Diagram(sequence.into()));
    let broken = VisualId::of("nope");
    visuals.want(broken, Source::Diagram("this is not a diagram\n".into()));
    let style = styled(0xff_ff_ff);
    // Two run at once; the third starts once one lands.
    for _ in 0..2 {
        let jobs = visuals.jobs(&style, &renderer);
        assert!(!jobs.is_empty() && jobs.len() <= 2);
        assert!(
            visuals.jobs(&style, &renderer).is_empty(),
            "the slots are taken"
        );
        for job in jobs {
            let (job_id, ticket, drawn) = job.run();
            assert!(visuals.finish(job_id, ticket, drawn));
        }
    }
    assert!(matches!(visuals.shown(other), Shown::Ready(_)));
    let Shown::Ready(drawn) = visuals.shown(id) else {
        panic!("the flowchart was not drawn");
    };
    assert!(drawn.width > 0. && drawn.height > 0.);
    let pixels = drawn.texture.as_bytes(0).unwrap();
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    assert!(matches!(
        visuals.shown(broken),
        Shown::Failed(Error::Diagram(_))
    ));

    // A theme change redraws the diagram; the old texture stays until the
    // new one lands, and an overtaken job's texture is released.
    let dark = styled(0x10_10_10);
    let jobs = visuals.jobs(&dark, &renderer);
    assert_eq!(jobs.len(), 2);
    assert!(matches!(visuals.shown(id), Shown::Ready(_)));
    let (job_id, ticket, drawn) = jobs
        .into_iter()
        .map(|job| job.run())
        .find(|(_, _, drawn)| drawn.is_ok())
        .unwrap();
    assert!(!visuals.finish(job_id, ticket + 100, drawn));
    assert_eq!(visuals.take_released().len(), 1);

    visuals.retain(&HashSet::new());
    assert!(matches!(visuals.shown(id), Shown::Pending));
    assert!(!visuals.take_released().is_empty());
}
