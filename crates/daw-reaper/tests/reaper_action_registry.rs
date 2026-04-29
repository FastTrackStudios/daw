//! Integration tests for the ActionRegistry service.
//!
//! Verifies that guest processes can register custom REAPER actions,
//! look them up, and check registration status.
//!
//! Run with:
//!
//!   cargo xtask reaper-test -- reaper_action_registry

use reaper_test::reaper_test;

#[reaper_test(isolated)]
async fn register_and_lookup_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    // Register a new action
    let cmd_id = actions
        .register("fts.test.register_lookup", "FTS Test: Register and Lookup")
        .await?;
    assert!(cmd_id > 0, "register should return a valid command ID");
    ctx.log(&format!("Registered cmd_id={cmd_id}"));

    // Look it up by name
    let looked_up = actions
        .lookup_command_id("fts.test.register_lookup")
        .await?;
    assert_eq!(
        looked_up,
        Some(cmd_id),
        "lookup should return the same command ID"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn is_registered_returns_true_for_known_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    // Register an action
    let cmd_id = actions
        .register("fts.test.is_registered", "FTS Test: Is Registered")
        .await?;
    assert!(cmd_id > 0);

    // Check it has a command ID
    let exists = actions.is_registered("fts.test.is_registered").await?;
    assert!(exists, "action should be registered after register()");

    // Check it's actually in REAPER's action list (not just command ID registry)
    let in_list = actions.is_in_action_list("fts.test.is_registered").await?;
    assert!(
        in_list,
        "action should appear in REAPER's action list after register()"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn is_registered_returns_false_for_unknown_action(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let exists = actions
        .is_registered("fts.test.definitely_not_registered_xyz")
        .await?;
    assert!(!exists, "unknown action should not be registered");

    Ok(())
}

#[reaper_test(isolated)]
async fn lookup_returns_none_for_unknown_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let result = actions
        .lookup_command_id("fts.test.nonexistent_action_xyz")
        .await?;
    assert_eq!(result, None, "unknown action should return None");

    Ok(())
}

#[reaper_test(isolated)]
async fn register_same_action_twice_returns_same_id(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let id1 = actions
        .register("fts.test.idempotent", "FTS Test: Idempotent Registration")
        .await?;
    let id2 = actions
        .register("fts.test.idempotent", "FTS Test: Idempotent Registration")
        .await?;

    assert_eq!(
        id1, id2,
        "registering the same action twice should return the same ID"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn register_in_menu_returns_valid_id(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let cmd_id = actions
        .register_in_menu(
            "FTS_SESSION_TEST_MENU_REGISTER",
            "FTS Test: Menu Registration",
        )
        .await?;
    assert!(
        cmd_id > 0,
        "register_in_menu should return a valid command ID"
    );
    ctx.log(&format!("register_in_menu cmd_id={cmd_id}"));

    Ok(())
}

#[reaper_test(isolated)]
async fn register_in_menu_is_findable(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let cmd_id = actions
        .register_in_menu("FTS_TRANSPORT_TEST_MENU_FIND", "FTS Test: Menu Findable")
        .await?;
    assert!(cmd_id > 0);

    let looked_up = actions
        .lookup_command_id("FTS_TRANSPORT_TEST_MENU_FIND")
        .await?;
    assert_eq!(
        looked_up,
        Some(cmd_id),
        "lookup should return the same ID as register_in_menu"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn register_toggle_in_menu_is_findable_for_input_namespace(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let cmd_id = actions
        .register_toggle_in_menu(
            "FTS_INPUT_TEST_TOGGLE_NAMESPACE",
            "FTS Test: Input Namespace Toggle",
        )
        .await?;
    assert!(
        cmd_id > 0,
        "register_toggle_in_menu should return a valid command ID"
    );

    let looked_up = actions
        .lookup_command_id("FTS_INPUT_TEST_TOGGLE_NAMESPACE")
        .await?;
    assert_eq!(looked_up, Some(cmd_id));

    let in_list = actions
        .is_in_action_list("FTS_INPUT_TEST_TOGGLE_NAMESPACE")
        .await?;
    assert!(
        in_list,
        "FTS_INPUT_TEST_TOGGLE_NAMESPACE should appear in REAPER's action list"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn register_and_register_in_menu_are_idempotent(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    // First register without menu
    let id1 = actions
        .register(
            "FTS_SIGNAL_TEST_IDEMPOTENT_MENU",
            "FTS Test: Idempotent Menu",
        )
        .await?;
    assert!(id1 > 0);

    // Then register again with menu — should return the same ID
    let id2 = actions
        .register_in_menu(
            "FTS_SIGNAL_TEST_IDEMPOTENT_MENU",
            "FTS Test: Idempotent Menu",
        )
        .await?;

    assert_eq!(
        id1, id2,
        "register then register_in_menu for the same action should return the same ID"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn execute_command_runs_native_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    // 40029 = "Undo" — a safe, well-known REAPER action that never shows a dialog
    actions.execute_command(40029).await?;
    ctx.log("execute_command(40029) completed without error");

    Ok(())
}

#[reaper_test(isolated)]
async fn execute_named_action_for_registered_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    // Register an action first
    let cmd_id = actions
        .register("FTS_SYNC_TEST_EXEC_NAMED", "FTS Test: Execute Named Action")
        .await?;
    assert!(cmd_id > 0);

    // Execute it by name
    let result = actions
        .execute_named_action("FTS_SYNC_TEST_EXEC_NAMED")
        .await?;
    assert!(
        result,
        "execute_named_action should return true for a registered action"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn execute_named_action_for_unknown_returns_false(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    let result = actions
        .execute_named_action("FTS_NONEXISTENT_ACTION_XYZ_999")
        .await?;
    assert!(
        !result,
        "execute_named_action should return false for an unknown action"
    );

    Ok(())
}
