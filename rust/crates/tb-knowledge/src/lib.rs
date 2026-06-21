//! SSOT-Wissensbasis: lädt kuratierte Markdown-Dokumente (Frontmatter + Body)
//! aus zwei Namespaces und selektiert sie deterministisch per Frontmatter +
//! lexikalischem Scoring in einen Grounding-Prompt — KEIN RAG.

mod base;
mod doc;
mod grounding;

pub use base::KnowledgeBase;
pub use doc::{parse_doc, KnowledgeDoc, KnowledgeError, Namespace};
pub use grounding::{assemble_grounding, Grounding};
