//! What the view draws for an entry, prepared when the entry changes rather
//! than on every frame: agent text parsed as Markdown with its Mermaid
//! fences split out as diagrams, its HTML fences as pages to preview, and
//! its picture links as pictures, pictures asked of [`Visuals`], and a tool
//! call's output and file changes summarized.

use super::visual::{MAX_DIAGRAM_SOURCE, Source, VisualId, Visuals};
use crate::release_notes::{self, Line};
use herdr_chat_protocol::{EntryId, EntryKind, ToolContent, Transcript};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

/// Lines of a tool call's text output shown under it.
const TOOL_LINES: usize = 6;
/// Bytes of a diff's sides compared line by line; past them it is counted
/// as changed without its lines.
const DIFF_BYTES: usize = 512 * 1024;
/// How much of one answer is drawn: as much text as the program keeps for
/// an entry, with room for long code. A longer answer ends with a note.
const MESSAGE: release_notes::Limits = release_notes::Limits {
    body: 256 * 1024,
    lines: 2_000,
    line: 4_000,
};

/// What ends an answer cut at [`MESSAGE`].
const CUT: &str = "The rest of this answer is too long to show here; Copy takes all of it.";

/// An answer's Markdown lines, ending with a note when some are not shown;
/// Copy still takes the whole answer.
fn message_lines(text: &str) -> Rc<[Line]> {
    let (mut lines, cut) = release_notes::parse_within(text, MESSAGE);
    if cut {
        lines.push(release_notes::note(CUT));
    }
    lines.into()
}

/// Diagrams drawn per message; later fences stay code.
const DIAGRAMS: usize = 8;
/// Pictures shown under one tool call.
const TOOL_IMAGES: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffSummary {
    pub(crate) path: String,
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolSummary {
    pub(crate) diffs: Vec<DiffSummary>,
    /// The first lines of its text output.
    pub(crate) lines: Vec<String>,
    /// Pictures it returned, the first [`TOOL_IMAGES`] of them.
    pub(crate) images: Vec<VisualId>,
    /// Text lines left out.
    pub(crate) more: usize,
    pub(crate) terminals: usize,
}

/// One stretch of an agent message.
#[derive(Debug)]
pub(crate) enum Part {
    Text(Rc<[Line]>),
    /// A Mermaid fence, with its source as code lines for when it cannot be
    /// drawn.
    Diagram(VisualId, Rc<[Line]>),
    /// An HTML fence: a page the user may preview.
    Html {
        page: Rc<str>,
    },
    /// A picture on the agent's machine; herdr-acp adds it after the
    /// message, so the message only names it.
    Picture {
        alt: String,
    },
    /// A picture on the web, fetched once the user asks for it.
    Remote {
        id: VisualId,
        url: crate::browser::WebUrl,
        alt: String,
    },
}

#[derive(Debug)]
pub(crate) enum Body {
    Message(Vec<Part>),
    /// An agent's compaction summary, folded.
    Summary(super::summary::Summary),
    Image(VisualId),
    Tool(ToolSummary),
}

impl Body {
    fn visuals(&self) -> impl Iterator<Item = VisualId> + '_ {
        let (parts, images): (&[Part], &[VisualId]) = match self {
            Self::Message(parts) => (parts, &[]),
            Self::Summary(_) => (&[], &[]),
            Self::Image(id) => (&[], std::slice::from_ref(id)),
            Self::Tool(summary) => (&[], &summary.images),
        };
        parts
            .iter()
            .filter_map(|part| match part {
                Part::Diagram(id, _) | Part::Remote { id, .. } => Some(*id),
                Part::Text(_) | Part::Html { .. } | Part::Picture { .. } => None,
            })
            .chain(images.iter().copied())
    }
}

/// Bodies by entry, each with the revision it was prepared at, and the
/// pictures they draw.
#[derive(Default)]
pub(crate) struct Prepared {
    bodies: HashMap<EntryId, (u64, Rc<Body>)>,
    /// Files the transcript's tool calls changed, first change first, with
    /// their lines added and removed summed.
    changed: Vec<DiffSummary>,
    pub(crate) visuals: Visuals,
}

impl Prepared {
    pub(crate) fn body(&self, id: EntryId) -> Option<Rc<Body>> {
        self.bodies.get(&id).map(|(_, body)| body.clone())
    }

    pub(crate) fn changed(&self) -> &[DiffSummary] {
        &self.changed
    }

    /// The entries that draw `visual`, to measure again once it lands.
    pub(crate) fn drawing(&self, visual: VisualId) -> impl Iterator<Item = EntryId> + '_ {
        self.bodies
            .iter()
            .filter(move |(_, (_, body))| body.visuals().any(|id| id == visual))
            .map(|(id, _)| *id)
    }

    /// Prepares the entries whose revision moved and forgets those gone.
    pub(crate) fn update(&mut self, transcript: &Transcript, keys: &[(EntryId, u64)]) {
        let live: HashSet<EntryId> = keys.iter().map(|(id, _)| *id).collect();
        self.bodies.retain(|id, _| live.contains(id));
        for (entry, (_, revision)) in transcript.entries.iter().zip(keys) {
            if self
                .bodies
                .get(&entry.id)
                .is_some_and(|(prepared, _)| prepared == revision)
            {
                continue;
            }
            let visuals = &mut self.visuals;
            let body = match &entry.kind {
                EntryKind::Agent { text } => {
                    let (lines, cut) = release_notes::parse_within(text, MESSAGE);
                    match super::summary::summary(&lines) {
                        Some(mut summary) => {
                            if cut {
                                summary.lines.push(release_notes::note(CUT));
                            }
                            Body::Summary(summary)
                        }
                        None => Body::Message(message(text, visuals)),
                    }
                }
                EntryKind::Image { image, .. } => {
                    let id = VisualId::of(&image.data);
                    visuals.want(id, Source::Image(image.clone()));
                    Body::Image(id)
                }
                EntryKind::Tool(tool) => Body::Tool(summarize(&tool.content, visuals)),
                _ => continue,
            };
            self.bodies.insert(entry.id, (*revision, Rc::new(body)));
        }
        self.changed.clear();
        for entry in &transcript.entries {
            let Some((_, body)) = self.bodies.get(&entry.id) else {
                continue;
            };
            let Body::Tool(summary) = &**body else {
                continue;
            };
            for diff in &summary.diffs {
                match self
                    .changed
                    .iter_mut()
                    .find(|known| known.path == diff.path)
                {
                    Some(known) => {
                        known.added += diff.added;
                        known.removed += diff.removed;
                    }
                    None => self.changed.push(diff.clone()),
                }
            }
        }
        let live: HashSet<VisualId> = self
            .bodies
            .values()
            .flat_map(|(_, body)| body.visuals())
            .collect();
        self.visuals.retain(&live);
    }

    /// Starts over, as after a snapshot that may reuse entry ids.
    pub(crate) fn clear(&mut self) {
        self.bodies.clear();
        self.changed.clear();
        self.visuals.clear();
    }
}

/// An agent message as Markdown text with its diagrams, pages, and
/// pictures between it. Only a closed fence becomes a diagram or a page, so
/// one still streaming shows as code.
fn message(text: &str, visuals: &mut Visuals) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut prose = String::new();
    let flush = |prose: &mut String, parts: &mut Vec<Part>| {
        if !prose.trim().is_empty() {
            parts.push(Part::Text(message_lines(prose)));
        }
        prose.clear();
    };
    let mut lines = text.split_inclusive('\n');
    let (mut diagrams, mut pages) = (0, 0);
    while let Some(line) = lines.next() {
        if let Some(part) = picture(line) {
            flush(&mut prose, &mut parts);
            parts.push(part);
            continue;
        }
        let fence = line.trim().strip_prefix("```").map(str::trim);
        let kind = match fence {
            Some(info) if info.eq_ignore_ascii_case("mermaid") && diagrams < DIAGRAMS => {
                Fence::Diagram
            }
            Some(info) if info.eq_ignore_ascii_case("html") && pages < PAGES => Fence::Page,
            _ => {
                prose.push_str(line);
                continue;
            }
        };
        let mut source = String::new();
        let mut closed = false;
        let mut fence = line.to_owned();
        for inner in lines.by_ref() {
            fence.push_str(inner);
            if inner.trim() == "```" {
                closed = true;
                break;
            }
            source.push_str(inner);
        }
        let limit = match kind {
            Fence::Diagram => MAX_DIAGRAM_SOURCE,
            Fence::Page => MAX_PAGE,
        };
        if !closed || source.trim().is_empty() || source.len() > limit {
            prose.push_str(&fence);
            continue;
        }
        flush(&mut prose, &mut parts);
        match kind {
            Fence::Diagram => {
                diagrams += 1;
                let id = VisualId::of(&source);
                let code = release_notes::parse(&fence).into();
                visuals.want(id, Source::Diagram(source.into()));
                parts.push(Part::Diagram(id, code));
            }
            Fence::Page => {
                pages += 1;
                parts.push(Part::Html {
                    page: source.into(),
                });
            }
        }
    }
    if !prose.trim().is_empty() || parts.is_empty() {
        parts.push(Part::Text(message_lines(&prose)));
    }
    parts
}

/// What a closed fence becomes.
enum Fence {
    Diagram,
    Page,
}

/// Pages per message; later HTML fences stay code.
const PAGES: usize = 4;
/// Bytes of an HTML fence shown as a page; a longer one stays code.
pub(crate) const MAX_PAGE: usize = 256 * 1024;

/// A line that is only a Markdown picture, `![alt](target)`: a web address
/// to fetch on request, or a file herdr-acp shows after the message.
fn picture(line: &str) -> Option<Part> {
    let inner = line.trim().strip_prefix("![")?.strip_suffix(')')?;
    let (alt, target) = inner.split_once("](")?;
    if alt.contains(['[', ']']) || target.contains(['(', ')']) {
        return None;
    }
    let target = target.trim();
    let target = target
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or_else(|| target.split_whitespace().next().unwrap_or(""));
    let alt = crate::notifications::safe_text(alt.trim(), 200);
    if target.is_empty() || target.starts_with("data:") {
        return None;
    }
    if target.starts_with("http://") || target.starts_with("https://") {
        let url = crate::browser::WebUrl::try_from(target).ok()?;
        return Some(Part::Remote {
            id: VisualId::of(url.as_str()),
            url,
            alt,
        });
    }
    // herdr-acp reads a path or a file URL; any other scheme stays text. A
    // Windows drive letter looks like a one-letter scheme.
    let scheme = target.split_once(':').map(|(scheme, _)| scheme);
    if scheme.is_some_and(|scheme| {
        scheme.len() > 1
            && scheme != "file"
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "+-.".contains(ch))
    }) {
        return None;
    }
    Some(Part::Picture { alt })
}

pub(crate) fn summarize(content: &[ToolContent], visuals: &mut Visuals) -> ToolSummary {
    let mut summary = ToolSummary::default();
    for item in content {
        match item {
            ToolContent::Text { text } => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    if summary.lines.len() < TOOL_LINES {
                        summary
                            .lines
                            .push(crate::notifications::safe_text(line, 400));
                    } else {
                        summary.more += 1;
                    }
                }
            }
            ToolContent::Diff { new, old, path } => {
                let (added, removed) = diff_counts(old.as_deref().unwrap_or(""), new);
                summary.diffs.push(DiffSummary {
                    path: path.clone(),
                    added,
                    removed,
                });
            }
            ToolContent::Image { image } => {
                if summary.images.len() < TOOL_IMAGES {
                    let id = VisualId::of(&image.data);
                    visuals.want(id, Source::Image(image.clone()));
                    summary.images.push(id);
                }
            }
            ToolContent::Terminal { .. } => summary.terminals += 1,
        }
    }
    summary
}

/// Lines added and removed between two versions of a file, by counting
/// each distinct line on both sides: what `+`/`-` totals show, without the
/// cost of an alignment.
pub(crate) fn diff_counts(old: &str, new: &str) -> (usize, usize) {
    if old.len() + new.len() > DIFF_BYTES {
        return (new.lines().count(), old.lines().count());
    }
    let mut counts: HashMap<&str, isize> = HashMap::new();
    for line in old.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    for line in new.lines() {
        *counts.entry(line).or_default() += 1;
    }
    counts
        .values()
        .fold((0, 0), |(added, removed), &count| match count {
            count if count > 0 => (added + count.unsigned_abs(), removed),
            count => (added, removed + count.unsigned_abs()),
        })
}
