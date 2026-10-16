use super::*;

#[test]
fn a_leftover_default_folder_is_not_a_second_default() {
    let root = fixture();
    // Older Herdr kept a session's files here; `default` is the root session.
    fs::create_dir(root.join("sessions/default")).unwrap();
    fs::write(root.join("sessions/default/plugins.json"), "{}").unwrap();
    fs::create_dir(root.join("sessions/fun")).unwrap();
    let sessions = list_with(&root, |_| false).unwrap();
    assert_eq!(names_of(&sessions), ["default", "fun"]);
    let _ = fs::remove_dir_all(&root);
}
