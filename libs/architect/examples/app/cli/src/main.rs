//! `app` — native CLI client for `app-server`.
//!
//! Drives the same `ExampleRepoClient` the web/wasm test crate uses,
//! over a real WebSocket. Argument parsing is figue.
//!
//! Examples:
//!
//! ```sh
//! app list                        # default server: ws://127.0.0.1:4040/vox
//! app create --name foo --description "first row"
//! app get <uuid>
//! app delete <uuid>
//! ```

use std::env;
use std::process::ExitCode;

use example::{ExampleCreate, ExampleRepoClient};
use facet::Facet;
use facet_pretty::FacetPretty;
use figue as args;
use owo_colors::OwoColorize;
use uuid::Uuid;
use vox_core::initiator_on;
use vox_websocket::WsLink;

#[derive(Facet, Debug)]
struct CommonArgs {
    /// Server vox URL.
    #[facet(args::named)]
    url: Option<String>,

    /// Verbose tracing output.
    #[facet(args::named, args::short = 'v')]
    verbose: bool,
}

#[derive(Facet, Debug)]
struct ListArgs {
    #[facet(args::named)]
    url: Option<String>,
    #[facet(args::named)]
    page_size: Option<u32>,
}

#[derive(Facet, Debug)]
struct GetArgs {
    #[facet(args::named)]
    url: Option<String>,
    #[facet(args::positional)]
    id: String,
}

#[derive(Facet, Debug)]
struct CreateArgs {
    #[facet(args::named)]
    url: Option<String>,
    #[facet(args::named)]
    name: String,
    #[facet(args::named)]
    description: Option<String>,
}

#[derive(Facet, Debug)]
struct DeleteArgs {
    #[facet(args::named)]
    url: Option<String>,
    #[facet(args::positional)]
    id: String,
}

const DEFAULT_URL: &str = "ws://127.0.0.1:4040/vox";

fn print_usage() {
    eprintln!(
        "{}

  app list [--url URL] [--page-size N]
  app get <uuid> [--url URL]
  app create --name NAME [--description DESC] [--url URL]
  app delete <uuid> [--url URL]

  Default URL: {DEFAULT_URL}
",
        "USAGE".bold()
    );
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    let argv_strs: Vec<&str> = argv.iter().map(String::as_str).collect();

    let result = match argv_strs.split_first() {
        None => {
            print_usage();
            return ExitCode::from(2);
        }
        Some((&"list", rest)) => run_list(parse(rest)).await,
        Some((&"get", rest)) => run_get(parse(rest)).await,
        Some((&"create", rest)) => run_create(parse(rest)).await,
        Some((&"delete", rest)) => run_delete(parse(rest)).await,
        Some((&"--help" | &"-h" | &"help", _)) => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some((cmd, _)) => {
            eprintln!("{}: unknown command `{cmd}`", "ERROR".red().bold());
            print_usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {e:#}", "ERROR".red().bold());
            ExitCode::FAILURE
        }
    }
}

fn parse<A: Facet<'static>>(argv: &[&str]) -> A {
    // figue's `DriverOutcome::unwrap` handles --help / --version / parse
    // errors with proper exit codes, so we don't need to.
    figue::from_slice(argv).unwrap()
}

async fn connect(url: Option<String>) -> eyre::Result<ExampleRepoClient> {
    let url = url.unwrap_or_else(|| DEFAULT_URL.into());
    let link = WsLink::connect(&url)
        .await
        .map_err(|e| eyre::eyre!("WsLink::connect({url}): {e:?}"))?;
    initiator_on(link)
        .establish::<ExampleRepoClient>()
        .await
        .map_err(|e| eyre::eyre!("vox handshake: {e:?}"))
}

async fn run_list(args: ListArgs) -> eyre::Result<()> {
    let client = connect(args.url).await?;
    use architect::Page;
    let page = client
        .list(
            Page {
                index: 0,
                size: args.page_size.unwrap_or(50),
            },
            None,
            None,
        )
        .await
        .map_err(|e| eyre::eyre!("list: {e:?}"))?;
    println!("{} examples", page.total);
    for ex in page.items {
        println!("  {}  {}", ex.id.to_string().dimmed(), ex.name);
    }
    Ok(())
}

async fn run_get(args: GetArgs) -> eyre::Result<()> {
    let client = connect(args.url).await?;
    let id: Uuid = args.id.parse().map_err(|e| eyre::eyre!("bad UUID: {e}"))?;
    let ex = client
        .get(id)
        .await
        .map_err(|e| eyre::eyre!("get: {e:?}"))?;
    println!("{}", ex.pretty());
    Ok(())
}

async fn run_create(args: CreateArgs) -> eyre::Result<()> {
    let client = connect(args.url).await?;
    let ex = client
        .create(ExampleCreate {
            name: args.name,
            description: args.description.unwrap_or_default(),
        })
        .await
        .map_err(|e| eyre::eyre!("create: {e:?}"))?;
    println!("{} {}", "created".green(), ex.id);
    Ok(())
}

async fn run_delete(args: DeleteArgs) -> eyre::Result<()> {
    let client = connect(args.url).await?;
    let id: Uuid = args.id.parse().map_err(|e| eyre::eyre!("bad UUID: {e}"))?;
    client
        .delete(id)
        .await
        .map_err(|e| eyre::eyre!("delete: {e:?}"))?;
    println!("{} {id}", "deleted".green());
    Ok(())
}

// `architect::Page` re-exported through `example::architect::Page` —
// pull from there to avoid a direct architect dep.
mod architect {
    pub use example::architect::Page;
}
