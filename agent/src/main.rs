mod agents;
mod gui;

use autoagents::llm::{backends::openai::OpenAI, builder::LLMBuilder};
use clap::{Parser, Subcommand};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run ClusterHostRuntime for coordinating all client connections
    Host {
        /// Port for the cluster host
        #[arg(short = 'p', long, default_value = "9000")]
        port: u16,
        /// Node name
        #[arg(short = 'n', long, default_value = "cluster_host")]
        name: String,
        /// Host address
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    /// Run DoctorAgent as cluster client with GUI
    Doctor {
        /// Port for this node
        #[arg(short = 'p', long, default_value = "9001")]
        port: u16,
        /// Cluster host address to connect to (e.g., localhost:9000)
        #[arg(long, default_value = "localhost:9000")]
        host_addr: String,
        /// Node name
        #[arg(short = 'n', long, default_value = "doctor")]
        name: String,
        /// Local host address
        #[arg(long, default_value = "localhost")]
        host: String,
    },
    /// Run AnalysisAgent as cluster client
    Analysis {
        /// Port for this node
        #[arg(short = 'p', long, default_value = "9002")]
        port: u16,
        /// Cluster host address to connect to (e.g., localhost:9000)
        #[arg(long, default_value = "localhost:9000")]
        host_addr: String,
        /// Node name
        #[arg(short = 'n', long, default_value = "analysis")]
        name: String,
        /// Local host address
        #[arg(long, default_value = "localhost")]
        host: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // Create LLM provider
    let llm = create_llm_provider()?;

    match args.command {
        Commands::Host { port, name, host } => {
            println!(
                "🏠 Starting Cluster Host on port {} with name {}",
                port, name
            );
            agents::run_cluster_host(name, port, host).await?;
        }
        Commands::Doctor {
            port,
            host_addr,
            name,
            host,
        } => {
            println!(
                "🔍 Starting Doctor Agent with GUI on port {} with name {}",
                port, name
            );

            // Create channels for communication
            let (response_tx, response_rx) = mpsc::unbounded_channel::<String>();
            let (user_tx, user_rx) = mpsc::unbounded_channel::<String>();

            // Start the agent in a separate thread
            let llm_clone = llm.clone();
            let name_clone = name.clone();
            let host_addr_clone = host_addr.clone();
            let host_clone = host.clone();
            let response_tx_clone = response_tx.clone();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    if let Err(e) = agents::run_doctor_agent(
                        llm_clone,
                        name_clone,
                        port,
                        host_addr_clone,
                        host_clone,
                        user_rx,
                        response_tx_clone,
                    )
                    .await
                    {
                        eprintln!("Agent error: {}", e);
                    }
                });
            });

            // Run the GUI
            gui::run_chat_app(user_tx, response_rx)?;
        }
        Commands::Analysis {
            port,
            host_addr,
            name,
            host,
        } => {
            println!(
                "🧠 Starting AnalysisAgent on port {} with name {}",
                port, name
            );
            agents::run_analysis_agent(llm, name, port, host_addr, host).await?;
        }
    }
    Ok(())
}

fn create_llm_provider() -> Result<Arc<OpenAI>, Box<dyn std::error::Error>> {
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");

    let llm: Arc<OpenAI> = LLMBuilder::<OpenAI>::new()
        .api_key(api_key)
        .model("gpt-4o-mini")
        .max_tokens(512)
        .temperature(0.2)
        .build()
        .expect("Failed to build LLM");

    Ok(llm)
}
