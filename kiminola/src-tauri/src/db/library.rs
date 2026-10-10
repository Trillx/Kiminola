//! The library: Spaces, the Meeting hierarchy and Library locations.
//!
//! Interface:
//! - [`LibraryIndex`] is a consistent in-memory view of every Space and
//!   Meeting placement. It resolves a Meeting's container path and renders the
//!   library tree, rejecting cycles and dangling references instead of hiding
//!   them.
//! - [`resolve_destination`] turns an optional [`LibraryLocation`] into the
//!   `(space_id, parent_meeting_id)` pair stored on a Meeting. `None` means the
//!   default Space, which is created on demand and remembered in settings.
//! - [`move_node`] is the only way to change a placement after creation. A
//!   Meeting always has exactly one direct container; Spaces only move into
//!   Spaces (or the root); nothing may move inside its own descendants.

use std::collections::{HashMap, HashSet};

use sqlx::SqlitePool;

use super::{now_iso, settings};

const DEFAULT_SPACE_KEY: &str = "default_space_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LibraryLocation {
    Space { id: i64 },
    Meeting { id: i64 },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LibraryNode {
    Space {
        id: i64,
        name: String,
        children: Vec<LibraryNode>,
    },
    Meeting {
        id: i64,
        title: String,
        created_at: String,
        duration_seconds: i64,
        children: Vec<LibraryNode>,
    },
}

#[derive(serde::Serialize)]
pub struct SpaceMeetingRef {
    pub id: i64,
    pub title: String,
}

#[derive(serde::Serialize)]
pub struct SpaceOut {
    pub id: i64,
    pub name: String,
    pub meetings: Vec<SpaceMeetingRef>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SpaceRow {
    id: i64,
    name: String,
    parent_id: Option<i64>,
}

/// A Meeting's identity and placement as stored in `meetings`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct MeetingRow {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) created_at: String,
    pub(crate) duration_seconds: i64,
    pub(crate) space_id: Option<i64>,
    pub(crate) parent_meeting_id: Option<i64>,
}

/// Where a Meeting lives, described by its container (never the Meeting itself).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Placement {
    /// Name of the Space that ultimately contains the Meeting.
    pub(crate) space_name: Option<String>,
    /// `"Space / Child Space / Parent Meeting"`, or `None` at the root.
    pub(crate) location_path: Option<String>,
}

pub(crate) struct LibraryIndex {
    spaces: HashMap<i64, SpaceRow>,
    /// Spaces in creation order, used for stable tree rendering.
    space_order: Vec<i64>,
    meetings: HashMap<i64, MeetingRow>,
    /// Meetings newest first, the order every list view uses.
    meeting_order: Vec<i64>,
}

impl LibraryIndex {
    pub(crate) async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let space_rows = sqlx::query_as::<_, SpaceRow>(
            "SELECT id, name, parent_id
             FROM spaces
             ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        let meeting_rows = sqlx::query_as::<_, MeetingRow>(
            "SELECT id, title, created_at, duration_seconds, space_id, parent_meeting_id
             FROM meetings
             ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(Self {
            space_order: space_rows.iter().map(|row| row.id).collect(),
            spaces: space_rows.into_iter().map(|row| (row.id, row)).collect(),
            meeting_order: meeting_rows.iter().map(|row| row.id).collect(),
            meetings: meeting_rows.into_iter().map(|row| (row.id, row)).collect(),
        })
    }

    pub(crate) fn meeting(&self, id: i64) -> Option<&MeetingRow> {
        self.meetings.get(&id)
    }

    /// Every Meeting, newest first.
    pub(crate) fn meetings(&self) -> impl Iterator<Item = &MeetingRow> {
        self.meeting_order.iter().map(|id| &self.meetings[id])
    }

    /// Resolve the container path of `meeting_id`, failing on cycles or
    /// references to missing rows.
    pub(crate) fn locate(&self, meeting_id: i64) -> Result<Placement, String> {
        let mut meeting_names = Vec::new();
        let mut current_meeting = Some(meeting_id);
        let mut root_space_id = None;
        let mut visited_meetings = HashSet::new();

        while let Some(id) = current_meeting {
            if !visited_meetings.insert(id) {
                return Err("meeting hierarchy contains a cycle".to_string());
            }
            let row = self
                .meetings
                .get(&id)
                .ok_or_else(|| "meeting hierarchy references a missing meeting".to_string())?;
            meeting_names.push(row.title.clone());
            if let Some(parent_id) = row.parent_meeting_id {
                current_meeting = Some(parent_id);
            } else {
                root_space_id = row.space_id;
                current_meeting = None;
            }
        }

        let mut space_names = Vec::new();
        let mut current_space = root_space_id;
        let mut visited_spaces = HashSet::new();
        while let Some(id) = current_space {
            if !visited_spaces.insert(id) {
                return Err("Space hierarchy contains a cycle".to_string());
            }
            let row = self
                .spaces
                .get(&id)
                .ok_or_else(|| "meeting hierarchy references a missing Space".to_string())?;
            space_names.push(row.name.clone());
            current_space = row.parent_id;
        }

        space_names.reverse();
        meeting_names.reverse();
        // A location path describes the container, not the meeting itself. This
        // keeps the same value useful in the home list, detail metadata, and
        // Markdown frontmatter.
        meeting_names.pop();
        let space_name = space_names.last().cloned();
        let mut segments = space_names;
        segments.extend(meeting_names);

        Ok(Placement {
            space_name,
            location_path: (!segments.is_empty()).then(|| segments.join(" / ")),
        })
    }

    /// Render the whole library as nested Spaces and Meetings.
    pub(crate) fn tree(&self) -> Result<Vec<LibraryNode>, String> {
        let mut child_spaces: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
        for id in &self.space_order {
            let parent_id = self.spaces[id]
                .parent_id
                .filter(|id| self.spaces.contains_key(id));
            child_spaces.entry(parent_id).or_default().push(*id);
        }

        let root_space_id = child_spaces.get(&None).and_then(|ids| ids.first().copied());
        let mut meetings_by_space: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut meetings_by_parent: HashMap<i64, Vec<i64>> = HashMap::new();
        for row in self.meetings() {
            if row.parent_meeting_id.is_some() && row.space_id.is_some() {
                return Err(format!(
                    "Meeting {} has both a Space and a parent Meeting",
                    row.id
                ));
            }
            if let Some(parent_id) = row.parent_meeting_id {
                if !self.meetings.contains_key(&parent_id) {
                    return Err(format!(
                        "Meeting {} references missing parent Meeting {}",
                        row.id, parent_id
                    ));
                }
                meetings_by_parent
                    .entry(parent_id)
                    .or_default()
                    .push(row.id);
            } else if let Some(space_id) = row.space_id.or(root_space_id) {
                if !self.spaces.contains_key(&space_id) {
                    return Err(format!(
                        "Meeting {} references missing Space {}",
                        row.id, space_id
                    ));
                }
                meetings_by_space.entry(space_id).or_default().push(row.id);
            } else {
                return Err("cannot place a meeting because no Space exists".to_string());
            }
        }

        // Validate every chain before rendering roots. Without this pass a cycle
        // that has another healthy root could otherwise disappear from the tree.
        for row in self.meetings() {
            let placement = self.locate(row.id)?;
            if row.parent_meeting_id.is_some() && placement.space_name.is_none() {
                return Err(format!("Meeting {} has no root Space", row.id));
            }
        }

        let mut builder = TreeBuilder {
            index: self,
            child_spaces: &child_spaces,
            meetings_by_space: &meetings_by_space,
            meetings_by_parent: &meetings_by_parent,
            visiting_spaces: HashSet::new(),
            visiting_meetings: HashSet::new(),
        };
        let mut tree = Vec::new();
        for id in child_spaces.get(&None).cloned().unwrap_or_default() {
            tree.push(builder.space(id)?);
        }
        if tree.is_empty() && !self.spaces.is_empty() {
            return Err("Space hierarchy contains a cycle".to_string());
        }
        Ok(tree)
    }
}

struct TreeBuilder<'a> {
    index: &'a LibraryIndex,
    child_spaces: &'a HashMap<Option<i64>, Vec<i64>>,
    meetings_by_space: &'a HashMap<i64, Vec<i64>>,
    meetings_by_parent: &'a HashMap<i64, Vec<i64>>,
    visiting_spaces: HashSet<i64>,
    visiting_meetings: HashSet<i64>,
}

impl TreeBuilder<'_> {
    fn space(&mut self, id: i64) -> Result<LibraryNode, String> {
        if !self.visiting_spaces.insert(id) {
            return Err("Space hierarchy contains a cycle".to_string());
        }
        let row = self
            .index
            .spaces
            .get(&id)
            .ok_or_else(|| "Space hierarchy references a missing Space".to_string())?;

        let mut children = Vec::new();
        for child_id in self
            .child_spaces
            .get(&Some(id))
            .cloned()
            .unwrap_or_default()
        {
            children.push(self.space(child_id)?);
        }
        for meeting_id in self.meetings_by_space.get(&id).cloned().unwrap_or_default() {
            children.push(self.meeting(meeting_id)?);
        }

        self.visiting_spaces.remove(&id);
        Ok(LibraryNode::Space {
            id: row.id,
            name: row.name.clone(),
            children,
        })
    }

    fn meeting(&mut self, id: i64) -> Result<LibraryNode, String> {
        if !self.visiting_meetings.insert(id) {
            return Err("meeting hierarchy contains a cycle".to_string());
        }
        let row = self
            .index
            .meetings
            .get(&id)
            .ok_or_else(|| "meeting hierarchy references a missing meeting".to_string())?;
        let child_ids = self
            .meetings_by_parent
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let mut children = Vec::with_capacity(child_ids.len());
        for child_id in child_ids {
            children.push(self.meeting(child_id)?);
        }
        self.visiting_meetings.remove(&id);
        Ok(LibraryNode::Meeting {
            id: row.id,
            title: row.title.clone(),
            created_at: row.created_at.clone(),
            duration_seconds: row.duration_seconds,
            children,
        })
    }
}

pub(crate) async fn list_tree(pool: &SqlitePool) -> Result<Vec<LibraryNode>, String> {
    LibraryIndex::load(pool).await?.tree()
}

/// Flat Space list with each Space's direct Meetings (newest first).
pub(crate) async fn list_spaces(pool: &SqlitePool) -> Result<Vec<SpaceOut>, String> {
    let spaces = sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM spaces ORDER BY id")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(spaces.len());
    for (id, name) in spaces {
        let meetings = sqlx::query_as::<_, (i64, String)>(
            "SELECT id, title FROM meetings WHERE space_id = ?
             ORDER BY created_at DESC, id DESC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        out.push(SpaceOut {
            id,
            name,
            meetings: meetings
                .into_iter()
                .map(|(id, title)| SpaceMeetingRef { id, title })
                .collect(),
        });
    }
    Ok(out)
}

pub(crate) async fn create_space(
    pool: &SqlitePool,
    name: &str,
    parent_space_id: Option<i64>,
) -> Result<i64, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Space name cannot be empty".to_string());
    }
    if let Some(parent_id) = parent_space_id {
        if !row_exists(pool, "SELECT id FROM spaces WHERE id = ?", parent_id).await? {
            return Err("parent Space not found".to_string());
        }
    }
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO spaces (name, parent_id, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(trimmed)
    .bind(parent_space_id)
    .bind(now_iso())
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())
}

pub(crate) async fn rename_space(
    pool: &SqlitePool,
    space_id: i64,
    name: &str,
) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Space name cannot be empty".to_string());
    }
    let rows = sqlx::query("UPDATE spaces SET name = ? WHERE id = ?")
        .bind(trimmed)
        .bind(space_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if rows == 0 {
        return Err("Space not found".to_string());
    }
    Ok(())
}

/// The Space new Meetings land in when no location is given. It survives
/// renames; if it was deleted, the first "Personal" Space (created if needed)
/// becomes the default again.
pub(crate) async fn default_space_id(pool: &SqlitePool) -> Result<i64, String> {
    if let Some(value) = settings::get(pool, DEFAULT_SPACE_KEY).await? {
        if let Ok(id) = value.parse::<i64>() {
            if row_exists(pool, "SELECT id FROM spaces WHERE id = ?", id).await? {
                return Ok(id);
            }
        }
    }

    let id = if let Some(id) = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM spaces WHERE name = 'Personal' ORDER BY id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    {
        id
    } else {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO spaces (name, created_at) VALUES ('Personal', ?) RETURNING id",
        )
        .bind(now_iso())
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?
    };

    settings::set(pool, DEFAULT_SPACE_KEY, &id.to_string()).await?;
    Ok(id)
}

/// Validate a requested Library location and return the
/// `(space_id, parent_meeting_id)` columns for a new Meeting.
pub(crate) async fn resolve_destination(
    pool: &SqlitePool,
    location: Option<LibraryLocation>,
) -> Result<(Option<i64>, Option<i64>), String> {
    match location {
        Some(LibraryLocation::Space { id }) => {
            if !row_exists(pool, "SELECT id FROM spaces WHERE id = ?", id).await? {
                return Err("destination Space not found".to_string());
            }
            Ok((Some(id), None))
        }
        Some(LibraryLocation::Meeting { id }) => {
            if !row_exists(pool, "SELECT id FROM meetings WHERE id = ?", id).await? {
                return Err("destination Meeting not found".to_string());
            }
            Ok((None, Some(id)))
        }
        None => Ok((Some(default_space_id(pool).await?), None)),
    }
}

async fn row_exists<'e, E>(executor: E, sql: &'static str, id: i64) -> Result<bool, String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    Ok(sqlx::query_scalar::<_, i64>(sql)
        .bind(id)
        .fetch_optional(executor)
        .await
        .map_err(|e| e.to_string())?
        .is_some())
}

/// Move a Space or Meeting to a new container in one transaction.
pub(crate) async fn move_node(
    pool: &SqlitePool,
    node: LibraryLocation,
    destination: Option<LibraryLocation>,
) -> Result<(), String> {
    const SPACE: &str = "SELECT id FROM spaces WHERE id = ?";
    const MEETING: &str = "SELECT id FROM meetings WHERE id = ?";

    if destination == Some(node) {
        return Err("an item cannot contain itself".to_string());
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    match (node, destination) {
        (LibraryLocation::Space { id }, None) => {
            if !row_exists(&mut *tx, SPACE, id).await? {
                return Err("Space not found".to_string());
            }
            sqlx::query("UPDATE spaces SET parent_id = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        (LibraryLocation::Space { id }, Some(LibraryLocation::Space { id: destination_id })) => {
            if !row_exists(&mut *tx, SPACE, id).await? {
                return Err("Space not found".to_string());
            }
            if !row_exists(&mut *tx, SPACE, destination_id).await? {
                return Err("destination Space not found".to_string());
            }
            let is_descendant: i64 = sqlx::query_scalar(
                "WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM spaces WHERE parent_id = ?
                     UNION ALL
                     SELECT spaces.id FROM spaces
                     JOIN descendants ON spaces.parent_id = descendants.id
                 )
                 SELECT EXISTS(SELECT 1 FROM descendants WHERE id = ?)",
            )
            .bind(id)
            .bind(destination_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            if is_descendant != 0 {
                return Err("a Space cannot move inside one of its descendants".to_string());
            }
            sqlx::query("UPDATE spaces SET parent_id = ? WHERE id = ?")
                .bind(destination_id)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        (LibraryLocation::Space { .. }, Some(LibraryLocation::Meeting { .. })) => {
            return Err("Spaces can only move into Spaces".to_string());
        }
        (LibraryLocation::Meeting { .. }, None) => {
            return Err("Meetings can only move into Spaces or Meetings".to_string());
        }
        (LibraryLocation::Meeting { id }, Some(LibraryLocation::Space { id: destination_id })) => {
            if !row_exists(&mut *tx, MEETING, id).await? {
                return Err("Meeting not found".to_string());
            }
            if !row_exists(&mut *tx, SPACE, destination_id).await? {
                return Err("destination Space not found".to_string());
            }
            sqlx::query(
                "UPDATE meetings
                 SET space_id = ?, parent_meeting_id = NULL
                 WHERE id = ?",
            )
            .bind(destination_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        (
            LibraryLocation::Meeting { id },
            Some(LibraryLocation::Meeting { id: destination_id }),
        ) => {
            if !row_exists(&mut *tx, MEETING, id).await? {
                return Err("Meeting not found".to_string());
            }
            if !row_exists(&mut *tx, MEETING, destination_id).await? {
                return Err("destination Meeting not found".to_string());
            }
            let is_descendant: i64 = sqlx::query_scalar(
                "WITH RECURSIVE descendants(id) AS (
                     SELECT id FROM meetings WHERE parent_meeting_id = ?
                     UNION ALL
                     SELECT meetings.id FROM meetings
                     JOIN descendants ON meetings.parent_meeting_id = descendants.id
                 )
                 SELECT EXISTS(SELECT 1 FROM descendants WHERE id = ?)",
            )
            .bind(id)
            .bind(destination_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            if is_descendant != 0 {
                return Err("a Meeting cannot move inside one of its descendants".to_string());
            }
            sqlx::query(
                "UPDATE meetings
                 SET space_id = NULL, parent_meeting_id = ?
                 WHERE id = ?",
            )
            .bind(destination_id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::meeting_store::{self, MeetingToSave};
    use crate::db::test_support::TestDb;

    async fn save_in(pool: &SqlitePool, title: &str, location: LibraryLocation) -> i64 {
        meeting_store::save_meeting(
            pool,
            MeetingToSave {
                title,
                location: Some(location),
                ..MeetingToSave::default()
            },
        )
        .await
        .expect("save Meeting")
    }

    async fn path_of(pool: &SqlitePool, id: i64) -> Option<String> {
        meeting_store::get_meeting(pool, id)
            .await
            .expect("get Meeting")
            .location_path
    }

    async fn personal_space(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT id FROM spaces WHERE name = 'Personal' ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .expect("seeded Personal Space")
    }

    #[tokio::test]
    async fn library_hierarchy_assigns_locations_and_validates_moves() {
        let db = TestDb::new("library-hierarchy").await;
        let pool = &db.pool;
        let personal_id = personal_space(pool).await;
        let work_id = create_space(pool, "Work", None).await.expect("create Work");
        let engineering_id = create_space(pool, "Engineering", Some(work_id))
            .await
            .expect("create Engineering child");

        let parent_id = save_in(pool, "Planning", LibraryLocation::Space { id: work_id }).await;
        let child_id = save_in(
            pool,
            "Follow-up",
            LibraryLocation::Meeting { id: parent_id },
        )
        .await;
        let grandchild_id =
            save_in(pool, "Decision", LibraryLocation::Meeting { id: child_id }).await;

        let invalid_raw_insert = sqlx::query(
            "INSERT INTO meetings (title, space_id, parent_meeting_id, created_at, duration_seconds)
             VALUES ('invalid', NULL, NULL, ?, 0)",
        )
        .bind(now_iso())
        .execute(pool)
        .await;
        assert!(
            invalid_raw_insert.is_err(),
            "database must enforce one location"
        );

        let parent = meeting_store::get_meeting(pool, parent_id).await.unwrap();
        assert_eq!(parent.space_name.as_deref(), Some("Work"));
        assert_eq!(parent.location_path.as_deref(), Some("Work"));
        let child = meeting_store::get_meeting(pool, child_id).await.unwrap();
        assert_eq!(child.parent_meeting_id, Some(parent_id));
        assert_eq!(child.location_path.as_deref(), Some("Work / Planning"));
        assert_eq!(
            path_of(pool, grandchild_id).await.as_deref(),
            Some("Work / Planning / Follow-up")
        );

        let tree_json = serde_json::to_string(&list_tree(pool).await.expect("list tree")).unwrap();
        for name in ["Engineering", "Planning", "Follow-up", "Decision"] {
            assert!(tree_json.contains(name), "{name} missing from tree");
        }

        // Meeting moves clear the old Space and preserve the entire child
        // subtree under the moved Meeting.
        move_node(
            pool,
            LibraryLocation::Meeting { id: child_id },
            Some(LibraryLocation::Space { id: engineering_id }),
        )
        .await
        .expect("move child to Space");
        let child = meeting_store::get_meeting(pool, child_id).await.unwrap();
        assert_eq!(child.space_name.as_deref(), Some("Engineering"));
        assert_eq!(child.location_path.as_deref(), Some("Work / Engineering"));
        assert_eq!(
            path_of(pool, grandchild_id).await.as_deref(),
            Some("Work / Engineering / Follow-up")
        );

        // Space reparenting changes the computed path without changing the
        // direct Meeting container.
        move_node(
            pool,
            LibraryLocation::Space { id: engineering_id },
            Some(LibraryLocation::Space { id: personal_id }),
        )
        .await
        .expect("reparent Engineering Space");
        assert_eq!(
            path_of(pool, child_id).await.as_deref(),
            Some("Personal / Engineering")
        );

        move_node(pool, LibraryLocation::Space { id: engineering_id }, None)
            .await
            .expect("move Engineering Space to library root");
        assert_eq!(
            path_of(pool, child_id).await.as_deref(),
            Some("Engineering")
        );

        move_node(
            pool,
            LibraryLocation::Space { id: engineering_id },
            Some(LibraryLocation::Space { id: personal_id }),
        )
        .await
        .expect("move Engineering Space back under Personal");

        move_node(
            pool,
            LibraryLocation::Meeting { id: child_id },
            Some(LibraryLocation::Meeting { id: parent_id }),
        )
        .await
        .expect("move child under parent Meeting");
        let child = meeting_store::get_meeting(pool, child_id).await.unwrap();
        assert_eq!(child.parent_meeting_id, Some(parent_id));
        assert_eq!(child.location_path.as_deref(), Some("Work / Planning"));

        let cycle_error = move_node(
            pool,
            LibraryLocation::Meeting { id: parent_id },
            Some(LibraryLocation::Meeting { id: grandchild_id }),
        )
        .await
        .expect_err("reject Meeting descendant cycle");
        assert!(cycle_error.contains("descendants"));

        let space_cycle = move_node(
            pool,
            LibraryLocation::Space { id: work_id },
            Some(LibraryLocation::Space { id: engineering_id }),
        )
        .await;
        // Engineering now lives under Personal, so this move is legal; moving
        // Personal under Engineering is the cycle.
        space_cycle.expect("move Work under Engineering");
        let space_cycle = move_node(
            pool,
            LibraryLocation::Space { id: personal_id },
            Some(LibraryLocation::Space { id: work_id }),
        )
        .await
        .expect_err("reject Space descendant cycle");
        assert!(space_cycle.contains("descendants"));

        let invalid_space_target = move_node(
            pool,
            LibraryLocation::Space { id: work_id },
            Some(LibraryLocation::Meeting { id: parent_id }),
        )
        .await
        .expect_err("reject Space into Meeting");
        assert!(invalid_space_target.contains("only move into Spaces"));

        let self_target = move_node(
            pool,
            LibraryLocation::Meeting { id: child_id },
            Some(LibraryLocation::Meeting { id: child_id }),
        )
        .await
        .expect_err("reject self containment");
        assert!(self_target.contains("cannot contain itself"));
    }

    #[tokio::test]
    async fn saving_into_a_missing_container_is_rejected() {
        let db = TestDb::new("library-missing-destination").await;
        for (location, expected) in [
            (
                LibraryLocation::Space { id: 404 },
                "destination Space not found",
            ),
            (
                LibraryLocation::Meeting { id: 404 },
                "destination Meeting not found",
            ),
        ] {
            let error = meeting_store::save_meeting(
                &db.pool,
                MeetingToSave {
                    title: "Orphan",
                    location: Some(location),
                    ..MeetingToSave::default()
                },
            )
            .await
            .expect_err("missing destination");
            assert_eq!(error, expected);
        }
        assert!(meeting_store::list_meetings(&db.pool)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn renamed_default_space_remains_the_recording_fallback() {
        let db = TestDb::new("renamed-default-space").await;
        let default_id = personal_space(&db.pool).await;

        rename_space(&db.pool, default_id, "Home")
            .await
            .expect("rename default Space");
        assert_eq!(default_space_id(&db.pool).await.unwrap(), default_id);

        let id = meeting_store::save_meeting(
            &db.pool,
            MeetingToSave {
                title: "Unfiled",
                ..MeetingToSave::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(path_of(&db.pool, id).await.as_deref(), Some("Home"));
    }

    #[tokio::test]
    async fn space_names_are_trimmed_and_required() {
        let db = TestDb::new("library-space-names").await;
        assert!(create_space(&db.pool, "   ", None).await.is_err());
        assert!(create_space(&db.pool, "Child", Some(404)).await.is_err());
        let id = create_space(&db.pool, "  Clients  ", None).await.unwrap();
        assert!(rename_space(&db.pool, id, "").await.is_err());
        assert!(rename_space(&db.pool, 404, "Name").await.is_err());
        let spaces = list_spaces(&db.pool).await.unwrap();
        assert!(spaces
            .iter()
            .any(|space| space.id == id && space.name == "Clients"));
    }
}
