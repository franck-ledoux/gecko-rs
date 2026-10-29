//! Groups of blocks: what they carry, and what carries them.

use gecko_cmd::{Command, Session};

fn stats(s: &Session) -> serde_json::Value {
    serde_json::from_str(&s.stats_json()).unwrap()
}

fn groups(s: &Session) -> Vec<serde_json::Value> {
    serde_json::from_str(&s.groups_json()).unwrap()
}

fn blocks(s: &Session) -> Vec<u32> {
    serde_json::from_str::<Vec<serde_json::Value>>(&s.blocks_json())
        .unwrap()
        .into_iter()
        .map(|b| b["id"].as_u64().unwrap() as u32)
        .collect()
}

/// Two cubes side by side, not glued: two blocks to put in groups.
fn pair() -> Session {
    let mut s = Session::new().unwrap();
    for at in [0.0, 2.0] {
        s.apply(Command::CreateCube {
            origin: [at, 0.0, 0.0],
            size: 1.0,
        })
        .expect("a cube");
    }
    s
}

#[test]
fn a_group_is_made_by_naming_it_and_found_by_the_same_name() {
    // Create-or-find: a script that says the same name twice means one
    // group both times, and the interface has nowhere sensible to put the
    // difference.
    let mut s = pair();
    let all = blocks(&s);

    let out = s
        .apply(Command::Group {
            name: "fluid".to_string(),
            blocks: vec![all[0]],
            written: true,
        })
        .expect("a group");
    assert!(out.message.contains("fluid"), "{}", out.message);
    assert_eq!(groups(&s).len(), 1);
    assert_eq!(groups(&s)[0]["blocks"], 1);

    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: vec![all[1]],
        written: true,
    })
    .unwrap();
    assert_eq!(groups(&s).len(), 1, "the same name is the same group");
    assert_eq!(groups(&s)[0]["blocks"], 2);

    // And a block can be in several at once.
    s.apply(Command::Group {
        name: "inlet".to_string(),
        blocks: vec![all[0]],
        written: false,
    })
    .unwrap();
    assert_eq!(groups(&s).len(), 2);
    assert_eq!(groups(&s)[1]["written"], false);
}

#[test]
fn a_name_nobody_gave_is_refused_rather_than_guessed() {
    let mut s = pair();
    let all = blocks(&s);
    for bad in [
        Command::Ungroup {
            name: "fluid".to_string(),
            blocks: vec![all[0]],
        },
        Command::RenameGroup {
            name: "fluid".to_string(),
            to: "solid".to_string(),
        },
        Command::DropGroup {
            name: "fluid".to_string(),
        },
    ] {
        let said = s.apply(bad).expect_err("no such group").to_string();
        assert!(said.contains("fluid"), "{said}");
    }

    // Nor a group with no name to ask for it by.
    let said = s
        .apply(Command::Group {
            name: "   ".to_string(),
            blocks: vec![],
            written: true,
        })
        .expect_err("unnamed")
        .to_string();
    assert!(said.contains("called something"), "{said}");
}

#[test]
fn cutting_a_block_puts_both_halves_in_what_it_was_in() {
    // The reason membership lives on the block's own attribute: the map
    // carries it through its operations, and nothing has to remember to.
    let mut s = pair();
    let all = blocks(&s);
    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: vec![all[0]],
        written: true,
    })
    .unwrap();

    let edge = serde_json::from_str::<Vec<serde_json::Value>>(&s.edges_json()).unwrap()[0]["id"]
        .as_u64()
        .unwrap() as u32;
    s.apply(Command::SplitSheet { edge, at: 0.5 })
        .expect("a cut");

    assert_eq!(stats(&s)["blocks"], 3, "one of the two became two");
    assert_eq!(groups(&s)[0]["blocks"], 2, "and both halves are in it");
}

#[test]
fn taking_a_block_out_leaves_the_group_and_dropping_the_group_leaves_the_blocks() {
    let mut s = pair();
    let all = blocks(&s);
    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: all.clone(),
        written: true,
    })
    .unwrap();
    assert_eq!(groups(&s)[0]["blocks"], 2);

    s.apply(Command::Ungroup {
        name: "fluid".to_string(),
        blocks: vec![all[0]],
    })
    .expect("out of it");
    assert_eq!(groups(&s)[0]["blocks"], 1, "the group is still there");
    assert_eq!(stats(&s)["blocks"], 2, "and so are the blocks");

    s.apply(Command::DropGroup {
        name: "fluid".to_string(),
    })
    .expect("dropped");
    assert!(groups(&s).is_empty(), "the group is gone");
    assert_eq!(stats(&s)["blocks"], 2, "the blocks are not");
}

#[test]
fn renaming_touches_no_block_and_refuses_a_name_in_use() {
    let mut s = pair();
    let all = blocks(&s);
    for (name, block) in [("fluid", all[0]), ("solid", all[1])] {
        s.apply(Command::Group {
            name: name.to_string(),
            blocks: vec![block],
            written: true,
        })
        .unwrap();
    }

    let said = s
        .apply(Command::RenameGroup {
            name: "fluid".to_string(),
            to: "solid".to_string(),
        })
        .expect_err("that name is taken")
        .to_string();
    assert!(said.contains("already called"), "{said}");

    s.apply(Command::RenameGroup {
        name: "fluid".to_string(),
        to: "water".to_string(),
    })
    .expect("renamed");
    let now = groups(&s);
    assert_eq!(now[0]["name"], "water");
    assert_eq!(now[0]["blocks"], 1, "its blocks did not move");
}

#[test]
fn a_group_comes_back_with_an_undo() {
    let mut s = pair();
    let all = blocks(&s);
    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: all,
        written: true,
    })
    .unwrap();
    s.apply(Command::DropGroup {
        name: "fluid".to_string(),
    })
    .unwrap();
    assert!(groups(&s).is_empty());

    // The name and the membership go back together: they live in two
    // places and one without the other is a group nobody can ask for.
    s.apply(Command::Undo).expect("an undo");
    assert_eq!(groups(&s).len(), 1);
    assert_eq!(groups(&s)[0]["blocks"], 2);
}

#[test]
fn the_mesh_carries_a_column_per_written_group() {
    let mut s = pair();
    let all = blocks(&s);
    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: vec![all[0]],
        written: true,
    })
    .unwrap();
    // A group somebody made to look at part of the model is not part of
    // the model: it stays out of the file.
    s.apply(Command::Group {
        name: "today".to_string(),
        blocks: vec![all[1]],
        written: false,
    })
    .unwrap();
    s.apply(Command::Mesh { segments: 2 }).expect("a mesh");

    let file = s.mesh_vtk().expect("a file");
    assert!(file.contains("SCALARS fluid int 1"), "{file}");
    assert!(!file.contains("today"), "a temporary group is not written");

    // Eight cells a block, and the first block's eight are the ones in it.
    let column: Vec<&str> = file
        .split("SCALARS fluid int 1\nLOOKUP_TABLE default\n")
        .nth(1)
        .expect("the column")
        .lines()
        .take(16)
        .collect();
    assert_eq!(column.iter().filter(|x| **x == "1").count(), 8);
    assert_eq!(column.iter().filter(|x| **x == "0").count(), 8);
}

#[test]
fn a_session_writes_its_groups_down_as_lua() {
    let mut s = pair();
    let all = blocks(&s);
    s.apply(Command::Group {
        name: "fluid".to_string(),
        blocks: vec![all[0]],
        written: true,
    })
    .unwrap();
    s.apply(Command::Group {
        name: "today".to_string(),
        blocks: vec![all[1]],
        written: false,
    })
    .unwrap();
    s.apply(Command::RenameGroup {
        name: "today".to_string(),
        to: "tomorrow".to_string(),
    })
    .unwrap();

    let script = s.script();
    assert!(script.contains(r#"gecko.group("fluid", {"#), "{script}");
    assert!(script.contains("written = false"), "{script}");
    assert!(
        script.contains(r#"gecko.rename_group("today", "tomorrow")"#),
        "{script}"
    );
}
