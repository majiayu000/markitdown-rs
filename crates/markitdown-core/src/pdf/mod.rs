//! PDF to Markdown as markitdown 0.1.9 does it: pdfplumber word
//! positions to detect form-like pages (rendered as aligned tables),
//! otherwise pdfminer.six's `extract_text()` with the default layout
//! analysis. Both are ported here rather than wrapped.

mod cmap;
mod cmap_index;
mod crypt;
mod document;
mod encoding;
mod filters;
mod font;
mod interp;
mod layout;
mod lexer;
mod metrics;
mod object;
mod page;
mod plumber;
mod tables;
mod truetype;
mod xref;

use crate::{Error, StreamInfo, pystr};
use document::Document;
use font::Fonts;

pub fn accepts(_data: &[u8], info: &StreamInfo) -> bool {
    let mimetype = info.mimetype.as_deref().unwrap_or("").to_lowercase();
    let extension = info.extension.as_deref().unwrap_or("").to_lowercase();
    extension == ".pdf"
        || mimetype.starts_with("application/pdf")
        || mimetype.starts_with("application/x-pdf")
}

pub fn convert(data: &[u8]) -> Result<String, Error> {
    let fail = |e: object::PdfError| Error::Conversion(format!("PDF: {e}"));
    let doc = Document::new(data).map_err(fail)?;
    let pages = doc.pages().map_err(fail)?;
    let mut fonts = Fonts::default();
    let items = pages
        .iter()
        .map(|p| interp::process_page(&doc, &mut fonts, p))
        .collect::<object::Result<Vec<_>>>()
        .map_err(fail)?;
    let miner = || items.iter().map(layout::page_text).collect::<String>();
    let mut markdown = match plumber_pass(&doc, &pages, &items) {
        Ok(Some(md)) => md,
        _ => miner(),
    };
    if markdown.is_empty() {
        markdown = miner();
    }
    Ok(merge_partial_numbering(&markdown))
}

/// The pdfplumber loop; Ok(None) when no page looked like a form, and Err
/// when pdfplumber would raise (markitdown then uses pdfminer alone).
fn plumber_pass(
    doc: &Document,
    pages: &[page::Page],
    items: &[interp::PageItems],
) -> object::Result<Option<String>> {
    let geometry = pages
        .iter()
        .map(|p| plumber::Geometry::new(doc, p))
        .collect::<object::Result<Vec<_>>>()?;
    let mut chunks: Vec<String> = Vec::new();
    let mut form_pages = 0;
    for (g, it) in geometry.iter().zip(items) {
        let chars = g.chars(it);
        match plumber::form_content(&chars, g.width)? {
            Some(content) => {
                form_pages += 1;
                if !pystr::strip(&content).is_empty() {
                    chunks.push(content);
                }
            }
            None => {
                let text = plumber::extract_text(&chars);
                let text = pystr::strip(&text);
                if !text.is_empty() {
                    chunks.push(text.to_string());
                }
            }
        }
    }
    Ok((form_pages > 0).then(|| pystr::strip(&chunks.join("\n\n")).to_string()))
}

/// `_merge_partial_numbering_lines`: ".1" alone on a line joins the next
/// non-blank line.
fn merge_partial_numbering(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let stripped = pystr::strip(lines[i]);
        if plumber::is_partial_numbering(stripped) {
            let mut j = i + 1;
            while j < lines.len() && pystr::strip(lines[j]).is_empty() {
                j += 1;
            }
            if j < lines.len() {
                out.push(format!("{stripped} {}", pystr::strip(lines[j])));
                i = j + 1;
                continue;
            }
        }
        out.push(lines[i].to_string());
        i += 1;
    }
    out.join("\n")
}
