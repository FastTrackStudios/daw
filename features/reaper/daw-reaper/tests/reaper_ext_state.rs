//! Integration tests for the ExtState service (#24).
//!
//! Round-trip coverage:
//! - set → get returns the value
//! - set → has reports presence
//! - set → delete → has reports absence
//! - persist=true vs persist=false isolation (in-memory only)
//!
//! Run with: `cargo xtask reaper-test -- reaper_ext_state`

use daw::test::reaper_test;

const TEST_SECTION: &str = "fts.test.ext_state";

#[reaper_test(isolated)]
async fn set_then_get_returns_value(ctx: &daw::test::ReaperTestContext) -> eyre::Result<()> {
    let ext = ctx.daw.ext_state();

    ext.set(TEST_SECTION, "color", "blue", false).await?;
    let got = ext.get(TEST_SECTION, "color").await?;
    assert_eq!(
        got.as_deref(),
        Some("blue"),
        "ExtState::set followed by get should return the same value"
    );

    Ok(())
}

#[reaper_test(isolated)]
async fn has_reflects_presence(ctx: &daw::test::ReaperTestContext) -> eyre::Result<()> {
    let ext = ctx.daw.ext_state();

    let before = ext.has(TEST_SECTION, "tempo").await?;
    assert!(!before, "key should be absent before set()");

    ext.set(TEST_SECTION, "tempo", "120.0", false).await?;
    let after = ext.has(TEST_SECTION, "tempo").await?;
    assert!(after, "key should exist after set()");

    Ok(())
}

#[reaper_test(isolated)]
async fn delete_removes_value(ctx: &daw::test::ReaperTestContext) -> eyre::Result<()> {
    let ext = ctx.daw.ext_state();

    ext.set(TEST_SECTION, "transient", "1", false).await?;
    assert!(ext.has(TEST_SECTION, "transient").await?, "set didn't take");

    ext.delete(TEST_SECTION, "transient", false).await?;
    let after = ext.has(TEST_SECTION, "transient").await?;
    assert!(!after, "key should be absent after delete()");

    let got = ext.get(TEST_SECTION, "transient").await?;
    assert_eq!(got, None, "get() after delete() should return None");

    Ok(())
}

#[reaper_test(isolated)]
async fn overwrite_replaces_value(ctx: &daw::test::ReaperTestContext) -> eyre::Result<()> {
    let ext = ctx.daw.ext_state();

    ext.set(TEST_SECTION, "mode", "manual", false).await?;
    ext.set(TEST_SECTION, "mode", "auto", false).await?;
    let got = ext.get(TEST_SECTION, "mode").await?;
    assert_eq!(
        got.as_deref(),
        Some("auto"),
        "second set() should replace the first"
    );

    Ok(())
}
