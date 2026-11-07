use rust_browser_core::{
    Page, Profile,
    session::{DeferredTabs, SessionPlan},
};

#[test]
fn unfocused_snapshot_keeps_the_previously_selected_tab() {
    let mut profile = Profile::default();
    profile.active_tab = 2;
    let pages: Vec<_> = (0..3)
        .map(|i| {
            (
                false,
                Page {
                    url: format!("https://example.org/{i}"),
                    title: String::new(),
                },
            )
        })
        .collect();
    profile.remember_session(pages.clone());
    assert_eq!(profile.active_tab, 2);
    profile.remember_session(pages.into_iter().take(2));
    assert_eq!(profile.active_tab, 1);
}

#[test]
fn navigation_during_an_inflight_load_waits_for_commit_and_keeps_latest_target() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.track_loading(
        1,
        Page {
            url: "https://initial.example/".into(),
            title: "Initial".into(),
        },
    );
    assert!(tabs.retarget_waiting(
        1,
        Page {
            url: "https://next.example/".into(),
            title: "Next".into()
        }
    ));
    assert!(tabs.retarget_waiting(
        1,
        Page {
            url: "https://latest.example/".into(),
            title: "Latest".into()
        }
    ));
    assert!(tabs.did_navigate(1, "about:blank").is_none());
    assert_eq!(tabs.get(1).unwrap().title, "Latest");
    assert_eq!(
        tabs.did_navigate(1, "https://redirect.example/")
            .unwrap()
            .url,
        "https://latest.example/"
    );
    assert_eq!(tabs.get(1).unwrap().title, "Latest");
    assert!(tabs.did_navigate(1, "https://latest.example/").is_none());
    assert!(tabs.get(1).is_none());
}

#[test]
fn activation_before_placeholder_commit_waits_instead_of_losing_the_load() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.insert(
        1,
        Page {
            url: "https://example.org/".into(),
            title: "Saved".into(),
        },
    );
    assert!(
        tabs.begin_activation(1).is_none(),
        "the blank browsing context is not ready yet"
    );
    assert!(tabs.begin_activation(1).is_none());
    assert_eq!(tabs.get(1).unwrap().url, "https://example.org/");
    assert_eq!(
        tabs.did_navigate(1, "about:blank").unwrap().url,
        "https://example.org/"
    );
    assert!(tabs.begin_activation(1).is_none());
}

#[test]
fn saved_selection_order_and_titles_survive_lazy_planning() {
    let mut profile = Profile::default();
    profile.session = vec![
        "https://one.example/".into(),
        "https://two.example/".into(),
        "servo:newtab".into(),
    ];
    profile.active_tab = 1;
    profile.history.push(Page {
        url: profile.session[0].clone(),
        title: "Первая страница".into(),
    });
    let plan = SessionPlan::from_profile(&profile).unwrap();
    assert_eq!(plan.active, 1);
    assert_eq!(
        plan.pages
            .iter()
            .map(|p| p.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://one.example/",
            "https://two.example/",
            "servo:newtab"
        ]
    );
    assert_eq!(plan.pages[0].title, "Первая страница");
    assert_eq!(profile.session.len(), 3);
}

#[test]
fn early_snapshot_and_close_keep_initial_loading_page() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.track_loading(
        5,
        Page {
            url: "https://active.example/".into(),
            title: "Active".into(),
        },
    );
    tabs.did_navigate(5, "about:blank");
    assert!(tabs.begin_activation(5).is_none());
    let mut profile = Profile::default();
    profile.remember_session([(true, tabs.get(5).unwrap().clone())]);
    assert_eq!(profile.session, ["https://active.example/"]);
    assert_eq!(profile.active_tab, 0);
    assert_eq!(tabs.take(5).unwrap().url, "https://active.example/");
}

#[test]
fn selection_index_uses_the_same_nonempty_urls_as_the_saved_list() {
    let mut profile = Profile::default();
    profile.remember_session([
        (
            false,
            Page {
                url: String::new(),
                title: "Uncommitted popup".into(),
            },
        ),
        (
            false,
            Page {
                url: "https://one.example/".into(),
                title: "One".into(),
            },
        ),
        (
            true,
            Page {
                url: "https://two.example/".into(),
                title: "Two".into(),
            },
        ),
    ]);
    assert_eq!(profile.session.len(), 2);
    assert_eq!(profile.active_tab, 1);
}

#[test]
fn latest_navigation_replaces_a_target_waiting_for_blank_commit() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.insert(
        1,
        Page {
            url: "https://old.example/".into(),
            title: "Old".into(),
        },
    );
    tabs.begin_activation(1);
    assert!(tabs.retarget_waiting(
        1,
        Page {
            url: "https://new.example/".into(),
            title: "New".into()
        }
    ));
    assert_eq!(
        tabs.did_navigate(1, "about:blank").unwrap().url,
        "https://new.example/"
    );
    assert!(tabs.did_navigate(1, "about:blank").is_none());
}

#[test]
fn invalid_saved_urls_do_not_shift_the_selected_valid_tab() {
    let mut profile = Profile::default();
    profile.session = vec![
        "invalid".into(),
        "https://one.example/".into(),
        "javascript:alert(1)".into(),
        "https://two.example/".into(),
    ];
    profile.active_tab = 3;
    let plan = SessionPlan::from_profile(&profile).unwrap();
    assert_eq!(plan.pages.len(), 2);
    assert_eq!(plan.active, 1);
    profile.active_tab = usize::MAX;
    assert_eq!(SessionPlan::from_profile(&profile).unwrap().active, 1);
    profile.session = vec!["javascript:alert(1)".into(), "data:text/html,hello".into()];
    assert!(SessionPlan::from_profile(&profile).is_none());
}

#[test]
fn duplicate_saved_urls_remain_separate_tabs() {
    let mut profile = Profile::default();
    profile.session = vec!["https://one.example/".into(); 3];
    profile.active_tab = 2;
    let plan = SessionPlan::from_profile(&profile).unwrap();
    assert_eq!(plan.pages.len(), 3);
    assert_eq!(plan.active, 2);
}

#[test]
fn deferred_activation_is_once_per_tab_and_keeps_other_metadata() {
    let mut tabs = DeferredTabs::<u64>::default();
    let page = Page {
        url: "https://example.org/".into(),
        title: "Example".into(),
    };
    tabs.insert(1, page.clone());
    tabs.insert(2, page);
    tabs.did_navigate(1, "about:blank");
    assert_eq!(tabs.get(1).unwrap().title, "Example");
    assert_eq!(
        tabs.begin_activation(1).unwrap().url,
        "https://example.org/"
    );
    assert!(tabs.begin_activation(1).is_none());
    assert_eq!(tabs.get(1).unwrap().url, "https://example.org/");
    assert!(!tabs.is_deferred(1));
    assert!(tabs.is_deferred(2));
    tabs.did_navigate(1, "https://example.org/");
    assert!(tabs.get(1).is_none());
    assert_eq!(tabs.get(2).unwrap().url, "https://example.org/");
    assert_eq!(tabs.len(), 1);
}

#[test]
fn initial_blank_commit_does_not_lose_a_loading_target_or_redirect() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.insert(
        1,
        Page {
            url: "https://example.org/".into(),
            title: "Saved".into(),
        },
    );
    tabs.did_navigate(1, "about:blank");
    assert!(tabs.is_deferred(1));
    tabs.begin_activation(1).unwrap();
    tabs.did_navigate(1, "about:blank");
    assert_eq!(tabs.get(1).unwrap().title, "Saved");
    tabs.did_navigate(1, "https://redirect.example/");
    assert!(tabs.get(1).is_none());
}

#[test]
fn closing_deferred_tab_can_recover_its_real_page() {
    let mut tabs = DeferredTabs::<u64>::default();
    tabs.insert(
        7,
        Page {
            url: "https://example.org/".into(),
            title: "Saved".into(),
        },
    );
    let closed = tabs.take(7).unwrap();
    let mut profile = Profile::default();
    profile.remember_closed(&closed.url, &closed.title);
    assert_eq!(profile.take_closed().unwrap().title, "Saved");
    assert_eq!(tabs.len(), 0);
}
