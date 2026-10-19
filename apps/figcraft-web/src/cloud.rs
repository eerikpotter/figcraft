//! Native FigCraft archives over the shared cloud SDK. Other tabs stay open.
use figcraft_engine::{DocState, Session, branches};
use sovereign_cloud_ui::{DocumentState, Editor, Snapshot};
use std::hash::{Hash, Hasher};

pub struct Adapter<'a>(&'a mut Session);

impl<'a> Adapter<'a> {
    pub fn new(session: &'a mut Session) -> Self {
        Self(session)
    }

    fn file(&self) -> Result<&DocState, String> {
        self.0.docs.get(branches::file_tab(self.0)).ok_or_else(|| "No design is open".into())
    }

    fn ready(&self, id: u64) -> Result<(), String> {
        for d in self.0.docs.iter().filter(|d| belongs_to(d, id)) {
            if d.interaction.is_some() || d.text_edit.is_some() {
                return Err("Finish editing text or dragging before saving to cloud".into());
            }
            if d.preview.is_some() || d.text_preview.is_some() {
                return Err("Return to the current design before saving to cloud".into());
            }
        }
        Ok(())
    }

    fn revision(&self, file: &DocState) -> u64 {
        // A file includes its open branch tabs. This is a change token, never an
        // authorization or integrity proof; saved() also compares the exact archive.
        let mut stamp = std::collections::hash_map::DefaultHasher::new();
        for d in self.0.docs.iter().filter(|d| belongs_to(d, file.uid)) {
            (d.uid, d.revision).hash(&mut stamp);
        }
        stamp.finish()
    }

    fn snapshot(&self, file: &DocState) -> DocState {
        let mut file = file.clone();
        // Upstream syncs branches before the next command. A host save must also
        // include the latest edit when there has not been another command yet.
        for d in &self.0.docs {
            if let Some((main, id)) = &d.branch_of
                && *main == file.uid
                && let Some(branch) = file.branches.iter_mut().find(|b| b.id == *id)
            {
                branch.head = d.document_itself().clone();
                branch.versions = d.versions.clone();
            }
        }
        file
    }
}

fn belongs_to(d: &DocState, id: u64) -> bool {
    d.uid == id || d.branch_of.as_ref().is_some_and(|(main, _)| *main == id)
}

fn encode(file: &DocState) -> Result<Vec<u8>, String> {
    let mut doc = (**file.document_itself()).clone();
    doc.current_page = Some(file.page);
    figcraft_format::save_bytes_with_history(&doc, None, &file.versions, &file.branches).map_err(|e| e.to_string())
}

impl Editor for Adapter<'_> {
    fn set_user_profile(&mut self, profile: Option<sovereign_cloud_ui::UserProfile<'_>>) {
        let previous = self.0.prefs.user_id.is_some();
        self.0.prefs.user_id = profile.map(|p| p.id.to_owned());
        if let Some(profile) = profile {
            if self.0.prefs.user_name != profile.display_name {
                self.0.prefs.user_name = profile.display_name.to_owned();
            }
        } else if previous {
            self.0.prefs.user_name = "You".into();
        }
    }

    fn document(&self) -> Option<DocumentState> {
        let file = self.file().ok()?;
        Some(DocumentState {
            id: file.uid,
            revision: self.revision(file),
            name: file.document_itself().name.clone(),
            dirty: self.0.docs.iter().filter(|d| belongs_to(d, file.uid)).any(|d| d.is_dirty() || d.text_edit.is_some() || d.interaction.is_some()),
        })
    }

    fn capture(&self) -> Result<Snapshot, String> {
        let document = self.document().ok_or("No design is open")?;
        self.ready(document.id)?;
        let file = self.snapshot(self.file()?);
        Ok(Snapshot { document, encode: Box::new(move || encode(&file)) })
    }

    fn saved(&mut self, id: u64, revision: u64, bytes: &[u8]) -> Result<Option<u64>, String> {
        if self.ready(id).is_err() {
            return Ok(None);
        }
        let Some(file) = self.0.docs.iter().find(|d| d.uid == id) else { return Ok(None) };
        if self.revision(file) != revision {
            return Ok(None);
        }
        let snapshot = self.snapshot(file);
        // A native archive also contains history and branches, not just canvas edits.
        if encode(&snapshot)? != bytes {
            return Ok(None);
        }
        if let Some(file) = self.0.docs.iter_mut().find(|d| d.uid == id) {
            file.branches = snapshot.branches;
        }
        for d in self.0.docs.iter_mut().filter(|d| belongs_to(d, id)) {
            d.mark_saved();
        }
        Ok(Some(revision))
    }

    fn open(&mut self, _name: &str, bytes: &[u8]) -> Result<DocumentState, String> {
        let loaded = figcraft_format::load_bytes_with_versions(bytes).map_err(|e| e.to_string())?;
        // Cloud downloads must never silently repair away image data, history or branches.
        if !loaded.issues.is_empty() {
            return Err(format!("The design could not be opened without repairs: {}", loaded.issues.join("; ")));
        }
        let file = DocState::new(loaded.doc, None).with_versions(loaded.versions).with_branches(loaded.branches);
        self.0.docs.push(file);
        self.0.active = self.0.docs.len().saturating_sub(1);
        self.document().ok_or_else(|| "Could not open the design".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shape(s: &mut Session, x: f64) {
        s.execute("shape.rectangle", &json!({"x": x, "y": 10, "width": 120, "height": 80})).unwrap();
    }

    fn capture(s: &mut Session) -> (DocumentState, Vec<u8>) {
        let snapshot = Adapter::new(s).capture().unwrap();
        (snapshot.document, (snapshot.encode)().unwrap())
    }

    #[test]
    fn native_archive_keeps_images_versions_branches_and_existing_tabs() {
        let mut s = Session::new();
        shape(&mut s, 10.0);
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([40, 80, 120, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let image = png.into_inner();
        let (hash, _, _) = figcraft_format::add_image(s.doc_mut().unwrap(), image.clone()).unwrap();
        s.execute("file.saveVersion", &json!({"name": "Original"})).unwrap();
        s.execute("branch.create", &json!({"name": "Alternative"})).unwrap();
        shape(&mut s, 200.0);
        s.execute("file.saveVersion", &json!({"name": "Branch version"})).unwrap();
        let (before, bytes) = capture(&mut s);
        assert_eq!(before.id, s.docs[0].uid, "a branch saves its main file");
        let loaded = figcraft_format::load_bytes_with_versions(&bytes).unwrap();
        assert!(loaded.issues.is_empty());
        assert_eq!(*loaded.doc.images[&hash].bytes, image);
        assert_eq!(loaded.versions[0].name, "Original");
        assert_eq!(loaded.branches[0].name, "Alternative");
        assert_eq!(loaded.branches[0].versions[0].name, "Branch version");
        assert!(loaded.branches[0].head.node_count() > loaded.doc.node_count());
        let reopened = Adapter::new(&mut s).open("Design.figcraft", &bytes).unwrap();
        assert_ne!(reopened.id, before.id);
        assert_eq!(s.docs.len(), 3);
        assert!(s.docs[0].is_dirty());
        let roundtrip = figcraft_format::load_bytes_with_versions(&capture(&mut s).1).unwrap();
        assert_eq!(roundtrip.doc, loaded.doc);
        assert_eq!(roundtrip.versions[0].document().unwrap(), loaded.versions[0].document().unwrap());
        assert_eq!(roundtrip.branches[0].head, loaded.branches[0].head);
        assert_eq!(roundtrip.branches[0].versions[0].document().unwrap(), loaded.branches[0].versions[0].document().unwrap());
    }

    #[test]
    fn immutable_upload_never_cleans_later_edits_or_another_document() {
        let mut s = Session::new();
        shape(&mut s, 10.0);
        let snapshot = Adapter::new(&mut s).capture().unwrap();
        let before = snapshot.document;
        shape(&mut s, 200.0);
        let bytes = (snapshot.encode)().unwrap();
        assert!(figcraft_format::load_bytes(&bytes).unwrap().0.node_count() < s.doc().unwrap().node_count());
        assert_eq!(Adapter::new(&mut s).saved(before.id, before.revision, &bytes).unwrap(), None);
        assert!(s.ds().unwrap().is_dirty());
        let (current, bytes) = capture(&mut s);
        Adapter::new(&mut s).open("Other design.figcraft", &bytes).unwrap();
        shape(&mut s, 30.0);
        assert_eq!(Adapter::new(&mut s).saved(current.id, current.revision, &bytes).unwrap(), Some(current.revision));
        assert!(!s.docs[0].is_dirty());
        assert!(s.ds().unwrap().is_dirty());
    }

    #[test]
    fn branch_edits_advance_file_revision_and_late_save_keeps_them_dirty() {
        let mut s = Session::new();
        shape(&mut s, 10.0);
        s.execute("branch.create", &json!({"name": "Alternative"})).unwrap();
        let (first, bytes) = capture(&mut s);
        assert_eq!(Adapter::new(&mut s).saved(first.id, first.revision, &bytes).unwrap(), Some(first.revision));
        assert!(!s.docs.iter().any(DocState::is_dirty));
        shape(&mut s, 200.0);
        let changed = Adapter::new(&mut s).document().unwrap();
        assert_ne!(changed.revision, first.revision);
        assert_eq!(changed.id, first.id);
        assert!(changed.dirty);
        assert_eq!(Adapter::new(&mut s).saved(first.id, first.revision, &bytes).unwrap(), None);
        let (current, bytes) = capture(&mut s);
        s.execute("file.saveVersion", &json!({"name": "New branch version"})).unwrap();
        assert_eq!(Adapter::new(&mut s).saved(current.id, current.revision, &bytes).unwrap(), None);
        let (current, bytes) = capture(&mut s);
        assert_eq!(Adapter::new(&mut s).saved(current.id, current.revision, &bytes).unwrap(), Some(current.revision));
        assert!(!s.docs.iter().any(DocState::is_dirty));
    }

    #[test]
    fn invalid_open_and_newer_formats_preserve_work_and_in_progress_edits_cannot_save() {
        let mut s = Session::new();
        shape(&mut s, 10.0);
        let (before, bytes) = capture(&mut s);
        for bad in [b"not a zip".as_slice(), b"{\"version\":999}".as_slice()] {
            assert!(Adapter::new(&mut s).open("Invalid.figcraft", bad).is_err());
        }
        assert_eq!(s.docs.len(), 1);
        assert_eq!(Adapter::new(&mut s).document().unwrap().id, before.id);
        assert_eq!(capture(&mut s).1, bytes);
        s.begin_interaction("Dragging").unwrap();
        assert!(Adapter::new(&mut s).capture().is_err());
        assert_eq!(Adapter::new(&mut s).saved(before.id, before.revision, &bytes).unwrap(), None);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn account_profile_follows_host_and_cannot_restore_identity() {
        let mut app = figcraft_engine::Session::default();
        app.prefs.user_name = "Same display name".into();
        for id in [None, Some("account-a"), Some("account-a"), Some("account-b"), None] {
            Adapter::new(&mut app).set_user_profile(id.map(|id| sovereign_cloud_ui::UserProfile { id, display_name: "Subnet name" }));
            assert_eq!(app.prefs.user_id.as_deref(), id);
            if id.is_some() {
                assert_eq!(app.prefs.user_name, "Subnet name");
            }
        }
        Adapter::new(&mut app).set_user_profile(Some(sovereign_cloud_ui::UserProfile { id: "account-a", display_name: "Subnet name" }));
        let saved = serde_json::to_value(&app.prefs).unwrap();
        assert!(saved.get("user_id").is_none() && saved.get("userId").is_none());
        let forged = serde_json::json!({"user_id": "other-account", "userId": "other-account"});
        let restored: figcraft_engine::Prefs = serde_json::from_value(forged).unwrap();
        assert!(restored.user_id.is_none());
    }
}
