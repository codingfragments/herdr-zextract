//! In-memory [`Host`] for unit tests: canned pane texts, a tab listing,
//! and a record of every read and insert.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{Host, PaneInfo, ReadSource, SourceRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCall {
    pub pane_id: String,
    pub source: ReadSource,
    pub lines: Option<u32>,
}

pub struct FakeHost {
    pub source: SourceRef,
    /// Text per pane id. A pane listed in `tab` but missing here fails
    /// to read (as a pane closed mid-grab would).
    pub texts: HashMap<String, String>,
    /// What `list_tab_panes` returns, in order.
    pub tab: Vec<PaneInfo>,
    pub reads: RefCell<Vec<ReadCall>>,
    pub inserts: RefCell<Vec<(String, String)>>,
    /// When set, `insert_text` fails with this message.
    pub insert_error: Option<String>,
}

impl FakeHost {
    pub fn new(source_pane: &str, tab_id: &str) -> Self {
        Self {
            source: SourceRef {
                pane_id: source_pane.to_string(),
                tab_id: tab_id.to_string(),
            },
            texts: HashMap::new(),
            tab: Vec::new(),
            reads: RefCell::new(Vec::new()),
            inserts: RefCell::new(Vec::new()),
            insert_error: None,
        }
    }

    /// Add a pane with `text`, also listing it on the tab.
    pub fn with_pane(mut self, pane_id: &str, title: &str, text: &str) -> Self {
        self.texts.insert(pane_id.to_string(), text.to_string());
        self.tab.push(PaneInfo {
            pane_id: pane_id.to_string(),
            title: title.to_string(),
        });
        self
    }
}

impl Host for FakeHost {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn source(&self) -> &SourceRef {
        &self.source
    }

    fn read_pane(
        &self,
        pane_id: &str,
        source: ReadSource,
        lines: Option<u32>,
    ) -> Result<String, String> {
        self.reads.borrow_mut().push(ReadCall {
            pane_id: pane_id.to_string(),
            source,
            lines,
        });
        self.texts
            .get(pane_id)
            .cloned()
            .ok_or_else(|| format!("no such pane: {pane_id}"))
    }

    fn list_tab_panes(&self) -> Result<Vec<PaneInfo>, String> {
        Ok(self.tab.clone())
    }

    fn insert_text(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if let Some(e) = &self.insert_error {
            return Err(e.clone());
        }
        self.inserts
            .borrow_mut()
            .push((pane_id.to_string(), text.to_string()));
        Ok(())
    }
}
