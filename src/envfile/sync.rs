//! Brings a `.env` in line with its template: the template decides the keys, their order,
//! comments and annotations; the `.env` keeps its values. Keys only in the `.env` are kept
//! at the end, so a value is never lost.

use super::{Document, Entry, Item, Value};

#[derive(Debug)]
pub struct SyncResult {
    pub doc: Document,
    /// Template keys the `.env` did not have yet (now empty).
    pub added: Vec<String>,
    /// `.env` keys the template does not list (kept, moved to the end).
    pub extra: Vec<String>,
}

pub fn sync(template: &Document, env: &Document) -> SyncResult {
    let mut doc = Document {
        header: env.header.clone(),
        items: Vec::new(),
    };
    let mut added = Vec::new();
    for item in &template.items {
        let Item::Entry(from_template) = item else {
            doc.items.push(item.clone());
            continue;
        };
        let value = match env.get(&from_template.key) {
            Some(existing) => existing.value.clone(),
            None => {
                added.push(from_template.key.clone());
                Value::Empty
            }
        };
        doc.items.push(Item::Entry(Entry {
            key: from_template.key.clone(),
            value,
            comments: from_template.comments.clone(),
            flags: from_template.flags,
            line: 0,
        }));
    }

    let extras: Vec<&Entry> = env
        .entries()
        .filter(|entry| template.get(&entry.key).is_none())
        .collect();
    if !extras.is_empty() && !matches!(doc.items.last(), None | Some(Item::Blank)) {
        doc.items.push(Item::Blank);
    }
    let extra = extras.iter().map(|entry| entry.key.clone()).collect();
    doc.items
        .extend(extras.into_iter().map(|entry| Item::Entry(entry.clone())));

    SyncResult { doc, added, extra }
}

#[cfg(test)]
mod tests {
    use super::super::{parse, render};
    use super::*;

    #[test]
    fn template_decides_order_comments_and_annotations() {
        let template =
            parse("# Port\n# @plain\nPORT=8899\n\n# Host baru\nHOST=\n# @optional\nPASS=\n")
                .unwrap();
        let env = parse(
            "SULTRAKEY_APP=a\nSULTRAKEY_PUBLIC_KEY=age1x\nOLD=keep me\nHOST=enc:QQ==\n# old help\nPORT=1\n",
        )
        .unwrap();
        let result = sync(&template, &env);
        assert_eq!(result.added, ["PASS"]);
        assert_eq!(result.extra, ["OLD"]);
        assert_eq!(
            render(&result.doc),
            "SULTRAKEY_APP=a\nSULTRAKEY_PUBLIC_KEY=age1x\n# Port\n# @plain\nPORT=1\n\n\
             # Host baru\nHOST=enc:QQ==\n# @optional\nPASS=\n\nOLD='keep me'\n"
        );
    }

    #[test]
    fn new_env_from_template_has_empty_values() {
        let template = parse("# @plain\nPORT=8899\nHOST=localhost\n").unwrap();
        let result = sync(&template, &Document::default());
        assert_eq!(result.added, ["PORT", "HOST"]);
        assert!(result.extra.is_empty());
        assert_eq!(render(&result.doc), "# @plain\nPORT=\nHOST=\n");
    }

    #[test]
    fn already_in_sync_is_unchanged() {
        let text = "SULTRAKEY_APP=a\nSULTRAKEY_PUBLIC_KEY=age1x\n# @plain\nPORT=1\n";
        let env = parse(text).unwrap();
        let template = parse("# @plain\nPORT=8899\n").unwrap();
        let result = sync(&template, &env);
        assert!(result.added.is_empty() && result.extra.is_empty());
        assert_eq!(render(&result.doc), text);
    }
}
