use async_trait::async_trait;
use autoagents::core::actor::Topic;
use autoagents::core::agent::memory::SlidingWindowMemory;
use autoagents::core::agent::prebuilt::executor::{ReActAgentOutput, ReActExecutor};
use autoagents::core::agent::task::Task;
use autoagents::core::agent::{AgentBuilder, AgentDeriveT, AgentExecutor, Context, ExecutorConfig};
use autoagents::core::environment::Environment;
use autoagents::core::error::Error;
use autoagents::core::protocol::{Event, TaskResult};
use autoagents::core::runtime::{ClusterClientRuntime, ClusterHostRuntime};
use autoagents::core::runtime::{Runtime, TypedRuntime};
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents::llm::backends::openai::OpenAI;
use autoagents::llm::chat::{ChatMessage, ChatRole, MessageType};
use autoagents_derive::{ToolInput, agent, tool};
use colored::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::collections::HashSet;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};

#[derive(Serialize, Deserialize, ToolInput, Debug)]
pub struct PublishTopicToAnalysisArgs {
    #[input(description = "The query to submit to ECG analysis agent that the doctor wants.")]
    query: String,
}

#[tool(
    name = "ecg_analysis_tool",
    description = "Use this tool to publish a topic to the analysis agent which can get the ecg data and analysis, Once the query is submitted, you can respond back to the user that the analyssi will be coming shortly",
    input = PublishTopicToAnalysisArgs,
)]
struct PublishTopicToAnalysis {}

#[async_trait]
impl ToolRuntime for PublishTopicToAnalysis {
    async fn execute(&self, context: &Context, args: Value) -> Result<Value, ToolCallError> {
        println!("🔧 Tool call to publish to analysis agent");
        let typed_args: PublishTopicToAnalysisArgs = serde_json::from_value(args)?;
        let analysis_topic = Topic::<Task>::new("analysis_agent");

        println!(
            "🚀 Publishing query to analysis_agent topic: {}",
            typed_args.query
        );

        let task = Task::new(typed_args.query.clone());
        println!("📦 Created task for publishing: {:?}", task);

        println!("🔧 About to publish via context.publish() to cluster...");
        match context.publish(analysis_topic.clone(), task).await {
            Ok(_) => {
                println!(
                    "✅ Successfully published query to analysis agent on topic: {:?}",
                    analysis_topic
                );
                println!("📡 Message should now be distributed to remote cluster nodes");

                // Add a small delay to ensure the message is sent
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                Ok(serde_json::to_value(format!(
                    "Analysis request submitted: '{}'. The analysis will be processed shortly.",
                    typed_args.query
                ))
                .unwrap())
            }
            Err(e) => {
                eprintln!(
                    "❌ Failed to publish to analysis agent on topic {:?}: {}",
                    analysis_topic, e
                );
                Err(ToolCallError::from(
                    Box::new(e) as Box<dyn std::error::Error + Send + Sync>
                ))
            }
        }
    }
}

#[agent(
    name = "doctor_agent",
    description = "You are an expert ECG Doctor Agent using the ReAct (Reasoning + Acting) execution pattern. Your primary role is to help answer user queries about ECG analysis through systematic reasoning and tool usage.

    ## Core Capabilities
    You can:
    - Ask Analysis Agent to analyze ECG data using the ecg_analysis tool
    - Interpret analysis results and provide medical recommendations
    - Respond directly to users with analysis findings

    ## CRITICAL LOOP PREVENTION LOGIC
    **IMPORTANT**: If you receive a message that:
    - Starts with '###' or contains 'Analysis Report'
    - Contains 'Key Insights', 'Strategic Recommendations', or 'Actionable Next Steps'
    - Appears to be analysis results from another agent

    Then you should:
    1. **DO NOT** use the ecg_analysis_tool again
    2. **DIRECTLY RESPOND** to the user with the analysis results
    3. **PROVIDE** your medical interpretation of the findings
    4. **FORMAT** the response for the patient in a clear, understandable manner

    ## ReAct Execution Pattern
    As a ReAct agent, you follow this pattern for NEW user queries:
    1. **Thought**: Analyze what needs to be done and plan your approach
    2. **Action**: Use appropriate tools to gather information (ONLY for new user queries)
    3. **Observation**: Process the results from your tools
    4. **Repeat**: Continue until task is complete

    For ANALYSIS RESPONSES: Skip tools, respond directly to user.

    Remember: Distinguish between new user queries (use tools) and analysis responses (respond directly).",
    tools = [PublishTopicToAnalysis],
)]
#[derive(Clone)]
pub struct DoctorAgent {}

#[agent(
    name = "analysis_agent",
    description = "You are an analysis agent that receives a query related to the ecg reading and you must provide a recommendation based on the data.",
    tools = [],
)]
pub struct AnalysisAgent {}

impl ReActExecutor for DoctorAgent {}

#[async_trait]
impl AgentExecutor for AnalysisAgent {
    type Output = String;
    type Error = Error;

    fn config(&self) -> ExecutorConfig {
        ExecutorConfig { max_turns: 10 }
    }

    async fn execute(
        &self,
        task: &Task,
        context: Arc<Context>,
    ) -> Result<Self::Output, Self::Error> {
        println!("🧠 [AnalysisAgent] *** EXECUTE METHOD CALLED ***");
        println!(
            "🧠 [AnalysisAgent] Received research data for analysis: {}",
            task.prompt
        );
        println!("🧠 [AnalysisAgent] Task details: {:?}", task);

        // Skip self-test messages to avoid infinite loop
        if task.prompt == "SELF_TEST" {
            println!("🧠 [AnalysisAgent] Skipping SELF_TEST message");
            return Ok("Self-test completed successfully".to_string());
        }

        let mut messages = vec![ChatMessage {
            role: ChatRole::System,
            message_type: MessageType::Text,
            content: format!(
                "{} - > ECG Data Context: {}",
                context.config().description,
                "Add ECG"
            ),
        }];

        let analysis_prompt = format!(
            "{}

Based on this research data, provide:
1. Key insights and patterns identified
2. Strategic recommendations
3. Risk assessment and opportunities
4. Actionable next steps
5. Executive summary of findings

Provide a comprehensive analysis report.",
            task.prompt
        );

        let chat_msg = ChatMessage {
            role: ChatRole::User,
            message_type: MessageType::Text,
            content: analysis_prompt,
        };
        messages.push(chat_msg);

        let response = context
            .llm()
            .chat(&messages, None, context.config().output_schema.clone())
            .await?;
        let analysis_result = response.text().unwrap_or_default();

        println!("📈 [AnalysisAgent] Analysis completed!");
        println!("\n{}", "=".repeat(80));
        println!("🎯 FINAL ANALYSIS REPORT:");
        println!("{}", "=".repeat(80));
        println!("{}", analysis_result);
        println!("{}\n", "=".repeat(80));

        // Analysis is complete - the result will be captured by the event handling system
        context
            .publish(
                Topic::<Task>::new("analysis_response"),
                Task::new(analysis_result.clone()),
            )
            .await?;

        Ok(analysis_result)
    }
}

pub async fn run_doctor_agent(
    llm: Arc<OpenAI>,
    node_name: String,
    port: u16,
    host_addr: String,
    host: String,
    mut user_rx: mpsc::UnboundedReceiver<String>,
    response_tx: mpsc::UnboundedSender<String>,
) -> Result<(), Error> {
    println!(
        "🔍 Initializing DoctorAgent cluster client on port {}",
        port
    );

    let sliding_window_memory = Box::new(SlidingWindowMemory::new(50));
    let research_topic = Topic::<Task>::new("doctor_agent");
    let user_messages_topic = Topic::<Task>::new("user_messages"); // Separate topic for GUI messages

    // Create cluster client runtime for DoctorAgent - it will connect to dedicated cluster host
    let runtime = ClusterClientRuntime::new(
        "doctor_client".to_string(),
        host_addr.clone(),
        node_name,
        "cluster-cookie".to_string(),
        port,
        host,
    );

    let research_agent = DoctorAgent {};

    // Build and register DoctorAgent - subscribe to user_messages topic (not doctor_agent to avoid loops)
    let _ = AgentBuilder::new(research_agent)
        .with_llm(llm)
        .runtime(runtime.clone())
        .subscribe_topic(user_messages_topic.clone()) // "user_messages" topic for GUI user queries
        .subscribe_topic(Topic::<Task>::new("analysis_response")) // "analysis_response" topic for analysis results
        // DO NOT subscribe to "analysis_agent" topic - that's for AnalysisAgent only
        .with_memory(sliding_window_memory)
        .build()
        .await?;

    println!("🔍 DoctorAgent subscribed to topics: ['user_messages', 'analysis_response']");
    println!("🔍 DoctorAgent processes user messages from 'user_messages' topic (no loops)");
    println!("🔍 DoctorAgent receives analysis results from 'analysis_response' topic");

    // Create environment and set up event handling
    let mut environment = Environment::new(None);
    let _ = environment.register_runtime(runtime.clone()).await;

    let receiver = environment.take_event_receiver(None).await?;
    handle_events(receiver, response_tx.clone(), runtime.clone(), false);

    // Start the runtime and environment
    tokio::spawn(async move {
        if let Err(e) = environment.run().await {
            eprintln!("Environment error: {}", e);
        }
    });

    // Connection to host is handled automatically in ClusterClientRuntime
    println!(
        "🌐 ClusterClientRuntime will connect to cluster host at {}",
        host_addr
    );
    sleep(Duration::from_secs(2)).await;

    // Listen for user messages from the GUI - create agent tasks directly to avoid cluster loops
    let runtime_clone = runtime.clone();
    let user_messages_topic_clone = user_messages_topic.clone();
    tokio::spawn(async move {
        while let Some(message) = user_rx.recv().await {
            println!("📋 Received user message: {}", message);
            
            // Only process messages that start with "USER_SEND:" to identify actual send events
            if message.starts_with("USER_SEND:") {
                let actual_message = message.strip_prefix("USER_SEND:").unwrap_or(&message);
                println!("✉️ Processing user send event directly: {}", actual_message);
                
                // Use regular publish - we'll handle deduplication at the agent level
                if let Err(e) = runtime_clone
                    .publish(&user_messages_topic_clone, Task::new(actual_message.to_string()))
                    .await
                {
                    eprintln!("Failed to publish user message: {}", e);
                }
            } else {
                println!("🔇 Skipping non-send message: {}", message);
            }
        }
    });

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    println!("🔍 Shutting down ResearchAgent...");
    if let Err(e) = runtime.stop().await {
        eprintln!("Error stopping runtime: {}", e);
    }

    Ok(())
}

pub async fn run_analysis_agent(
    llm: Arc<OpenAI>,
    node_name: String,
    port: u16,
    host_addr: String,
    host: String,
) -> Result<(), Error> {
    println!(
        "🧠 Initializing AnalysisAgent cluster client on port {}",
        port
    );

    let sliding_window_memory = Box::new(SlidingWindowMemory::new(10));
    let analysis_topic = Topic::<Task>::new("analysis_agent");

    // Create cluster client runtime for AnalysisAgent - it will connect to dedicated cluster host
    let runtime = ClusterClientRuntime::new(
        "analysis_client".to_string(),
        host_addr.clone(),
        node_name,
        "cluster-cookie".to_string(),
        port,
        host,
    );

    let analysis_agent = AnalysisAgent {};

    // Build and register AnalysisAgent
    let _ = AgentBuilder::new(analysis_agent)
        .with_llm(llm)
        .runtime(runtime.clone())
        .subscribe_topic(analysis_topic.clone())
        .with_memory(sliding_window_memory)
        .build()
        .await?;

    // Create environment and set up event handling
    let mut environment = Environment::new(None);
    let _ = environment.register_runtime(runtime.clone()).await;

    let receiver = environment.take_event_receiver(None).await?;
    let (_dummy_tx, _) = mpsc::unbounded_channel::<String>();

    // Use the regular handle_events function but with specific AnalysisAgent debugging
    let (analysis_response_tx, _) = mpsc::unbounded_channel::<String>();
    println!("🧠 Setting up AnalysisAgent event handler...");
    handle_events(receiver, analysis_response_tx, runtime.clone(), true);

    // Start the runtime and environment
    tokio::spawn(async move {
        if let Err(e) = environment.run().await {
            eprintln!("Environment error: {}", e);
        }
    });

    // Connection to host is handled automatically in ClusterClientRuntime
    println!(
        "🌐 ClusterClientRuntime will connect to cluster host at {}",
        host_addr
    );

    println!("🧠 AnalysisAgent ready to receive research data for analysis...");
    println!("🧠 AnalysisAgent subscribed to topic: analysis_agent");
    println!("🧠 AnalysisAgent runtime: {:?}", runtime);

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    println!("🧠 Shutting down AnalysisAgent...");
    if let Err(e) = runtime.stop().await {
        eprintln!("Error stopping runtime: {}", e);
    }

    Ok(())
}

fn handle_events(
    mut event_stream: ReceiverStream<Event>,
    response_sender: mpsc::UnboundedSender<String>,
    _runtime: Arc<dyn Runtime>,
    is_analysis_agent: bool,
) {
    tokio::spawn(async move {
        let agent_type = if is_analysis_agent {
            "🧠 AnalysisAgent"
        } else {
            "🔍 DoctorAgent"
        };
        println!(
            "{} event handler started, waiting for events...",
            agent_type
        );

        while let Some(event) = event_stream.next().await {
            println!(
                "{}",
                format!("{} Received event: {:?}", agent_type, event).cyan()
            );
            match event {
                Event::NewTask { actor_id: _, task } => {
                    println!("{}", format!("📨 New TASK: {:?}", task).green());

                    // Only forward user-initiated tasks, not analysis results, to avoid infinite loops
                    if !is_analysis_agent {
                        // Check if this is an analysis result that should be sent directly to GUI
                        if task.prompt.starts_with("### ")
                            || task.prompt.contains("Analysis Report")
                            || task.prompt.contains("Key Insights")
                            || task.prompt.contains("Strategic Recommendations")
                            || task.prompt.contains("Executive Summary")
                            || task.prompt.contains("RESEARCH DATA FOR ANALYSIS")
                        {
                            println!("📊 Received analysis result, sending directly to GUI");
                            match response_sender.send(task.prompt) {
                                Ok(_) => {
                                    println!("✅ Successfully sent analysis result to GUI channel")
                                }
                                Err(e) => {
                                    eprintln!("❌ Failed to send analysis result to GUI: {}", e)
                                }
                            }
                        } else {
                            println!(
                                "🔄 Doctor agent received new user task, forwarding to agent: {}",
                                task.prompt
                            );
                            // This is a regular user query - let it be processed by the agent
                            // Don't send to GUI here, let the agent handle it
                        }
                    }
                }
                Event::ToolCallRequested {
                    id: _,
                    tool_name,
                    arguments: _,
                } => {
                    println!("{}", format!("📨 New TOOL CALL: {}", tool_name).green());
                }
                Event::TaskComplete {
                    result: TaskResult::Value(val),
                    ..
                } => {
                    println!(
                        "{}",
                        format!("🎯 Task completed with value: {:?}", val).blue()
                    );

                    // First try to parse as ReActAgentOutput
                    match serde_json::from_value::<ReActAgentOutput>(val.clone()) {
                        Ok(out) => {
                            println!(
                                "{}",
                                format!("✅ Agent Response (ReAct): {}", out.response).green()
                            );

                            // Send as-is if it's not JSON
                            println!("🚀 Sending raw response to GUI: {}", out.response);
                            match response_sender.send(out.response.clone()) {
                                Ok(_) => {
                                    println!("✅ Successfully sent raw response to GUI channel")
                                }
                                Err(e) => eprintln!("❌ Failed to send response to GUI: {}", e),
                            }
                        }
                        Err(_) => {
                            // Try to parse as string
                            match serde_json::from_value::<String>(val.clone()) {
                                Ok(out) => {
                                    println!(
                                        "{}",
                                        format!("✅ Agent Response (String): {}", out).green()
                                    );
                                    // Send directly to GUI channel instead of publishing to cluster
                                    println!("🚀 Sending string response directly to GUI: {}", out);
                                    if !is_analysis_agent {
                                        match response_sender.send(out) {
                                            Ok(_) => println!(
                                                "✅ Successfully sent string response to GUI channel"
                                            ),
                                            Err(e) => eprintln!(
                                                "❌ Failed to send string response to GUI: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
                _ => {
                    println!("{}", format!("🔄 Other event received").cyan());
                }
            }
        }
    });
}

pub async fn run_cluster_host(node_name: String, port: u16, host: String) -> Result<(), Error> {
    println!("🏠 Initializing ClusterHostRuntime on port {}", port);

    // Create cluster host runtime - this coordinates all client connections and routes events
    let runtime = ClusterHostRuntime::new(node_name, "cluster-cookie".to_string(), port, host);

    // Create environment and set up event handling
    let mut environment = Environment::new(None);
    let _ = environment.register_runtime(runtime.clone()).await;

    let receiver = environment.take_event_receiver(None).await?;
    let (dummy_tx, _) = mpsc::unbounded_channel::<String>();
    handle_events(receiver, dummy_tx, runtime.clone(), false);

    // Start the runtime and environment
    tokio::spawn(async move {
        if let Err(e) = environment.run().await {
            eprintln!("Environment error: {}", e);
        }
    });

    println!("🏠 ClusterHostRuntime ready to coordinate client connections and route events...");

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    println!("🏠 Shutting down ClusterHostRuntime...");
    if let Err(e) = runtime.stop().await {
        eprintln!("Error stopping runtime: {}", e);
    }

    Ok(())
}
