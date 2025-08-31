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
use std::collections::HashSet;
use std::fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
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

// Tool for doctor to request camera analysis
#[derive(Serialize, Deserialize, ToolInput, Debug)]
pub struct CameraAnalysisArgs {
    #[input(description = "The specific query or analysis request for the camera image")]
    query: String,
}

#[tool(
    name = "camera_analysis",
    description = "Request camera to capture and analyze an image based on user query",
    input = CameraAnalysisArgs,
)]
struct CameraAnalysisTool {}

#[async_trait]
impl ToolRuntime for CameraAnalysisTool {
    async fn execute(&self, context: &Context, args: Value) -> Result<Value, ToolCallError> {
        println!("📷 Tool call to request camera analysis");
        let typed_args: CameraAnalysisArgs = serde_json::from_value(args)?;
        let camera_topic = Topic::<Task>::new("camera_requests");

        println!(
            "🚀 Publishing camera analysis request: {}",
            typed_args.query
        );

        let task = Task::new(typed_args.query.clone());
        println!("📦 Created camera analysis task: {:?}", task);

        println!("🔧 About to publish via context.publish() to cluster...");
        match context.publish(camera_topic.clone(), task).await {
            Ok(_) => {
                println!(
                    "✅ Successfully published camera analysis request to topic: {:?}",
                    camera_topic
                );
                Ok(serde_json::to_value(format!(
                    "Camera analysis request submitted: {}",
                    typed_args.query
                ))
                .unwrap())
            }
            Err(e) => {
                eprintln!(
                    "❌ Failed to publish camera analysis request on topic {:?}: {}",
                    camera_topic, e
                );
                Err(ToolCallError::from(
                    Box::new(e) as Box<dyn std::error::Error + Send + Sync>
                ))
            }
        }
    }
}

// Camera agent for image analysis
#[agent(
    name = "camera_agent",
    description = "You are a Camera Analysis Agent that can capture and analyze images. When you receive a query, you should:
    2. Analyze the image based on the specific query received
    3. Provide detailed medical observations and findings
    4. Focus on any medical devices, conditions, or relevant visual information

    Always be thorough in your visual analysis and provide clear response."
)]
#[derive(Clone)]
pub struct CameraAgent {}

// Custom executor implementation for camera agent
#[async_trait]
impl AgentExecutor for CameraAgent {
    type Output = String;
    type Error = Error;

    fn config(&self) -> ExecutorConfig {
        ExecutorConfig::default()
    }

    async fn execute(&self, task: &Task, context: Arc<Context>) -> Result<String, Error> {
        let query = task.prompt.clone();

        println!("📷 CameraAgent received query: {}", query);

        // Create images directory if it doesn't exist
        let images_dir = "captured_images";
        if !std::path::Path::new(images_dir).exists() {
            std::fs::create_dir(images_dir).unwrap_or_else(|e| {
                eprintln!("Failed to create images directory: {}", e);
            });
        }

        // Generate unique filename with timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let output_path = format!("{}/medical_image_{}.jpg", images_dir, timestamp);

        println!("📷 Attempting to capture image...");

        let mut capture_success = false;

        // Try imagesnap first (most reliable on macOS)
        let imagesnap_result = Command::new("imagesnap")
            .arg("-q") // Quiet mode
            .arg(&output_path)
            .output();

        match imagesnap_result {
            Ok(result) => {
                if result.status.success() && fs::metadata(&output_path).is_ok() {
                    println!("✅ Captured image with ImageSnap");
                    capture_success = true;
                } else {
                    println!("❌ ImageSnap failed, trying FFmpeg...");
                }
            }
            Err(_) => {
                println!("❌ ImageSnap not available, trying FFmpeg...");
            }
        }

        // Fallback: Try using ffmpeg if imagesnap failed
        if !capture_success {
            let ffmpeg_result = Command::new("ffmpeg")
                .args(&[
                    "-f",
                    "avfoundation",
                    "-video_size",
                    "640x480",
                    "-framerate",
                    "30",
                    "-i",
                    "0", // Default camera
                    "-vframes",
                    "1",  // Capture only 1 frame
                    "-y", // Overwrite output file
                    &output_path,
                ])
                .output();

            match ffmpeg_result {
                Ok(result) => {
                    if result.status.success() && fs::metadata(&output_path).is_ok() {
                        println!("✅ Captured image with FFmpeg");
                        capture_success = true;
                    } else {
                        println!("❌ FFmpeg failed");
                    }
                }
                Err(_) => {
                    println!("❌ FFmpeg not available");
                }
            }
        }

        if !capture_success {
            // Return error result if capture failed
            return Ok("Camera capture failed - no image analysis available".to_string());
        }

        // Read the captured image into a buffer
        let image_buffer = match fs::read(&output_path) {
            Ok(buffer) => {
                println!("📖 Image loaded successfully ({} KB)", buffer.len() / 1024);
                buffer
            }
            Err(e) => {
                println!("❌ Failed to read image file: {}", e);
                return Ok("Image file could not be read".to_string());
            }
        };

        println!("🤖 Sending image to AI for analysis...");

        // Create chat messages for LLM
        let messages = vec![
            ChatMessage {
                role: ChatRole::System,
                message_type: MessageType::Text,
                content: "You are a medical camera analysis agent. Analyze the provided image and respond with detailed medical assessment. Focus on any medical devices, conditions, or relevant visual information.".to_string(),
            },
            ChatMessage {
                role: ChatRole::User,
                message_type: MessageType::Image((autoagents::llm::chat::ImageMime::JPEG, image_buffer)),
                content: format!("Please analyze this medical image and respond to this query: {}. Provide detailed findings.", query),
            },
        ];

        // Call LLM directly with chat messages
        match context.llm().chat(&messages, None, None).await {
            Ok(response) => {
                println!("✅ AI analysis completed");
                let response_text = response.to_string();
                println!("📋 Camera Analysis Result: {}", response_text);

                // Publish the camera analysis result back to the doctor
                let camera_response_topic = Topic::<Task>::new("camera_response");
                let response_task =
                    Task::new(format!("### Camera Analysis Result\n{}", response_text));

                match context
                    .publish(camera_response_topic.clone(), response_task)
                    .await
                {
                    Ok(_) => {
                        println!(
                            "✅ Successfully published camera analysis to doctor topic: {:?}",
                            camera_response_topic
                        );
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to publish camera analysis to doctor: {}", e);
                    }
                }

                Ok(response_text)
            }
            Err(e) => {
                println!("❌ LLM analysis failed: {}", e);
                let error_msg = format!("AI analysis failed: {}", e);

                // Publish the error back to the doctor as well
                let camera_response_topic = Topic::<Task>::new("camera_response");
                let error_task = Task::new(format!("### Camera Analysis Error\n{}", error_msg));

                let _ = context.publish(camera_response_topic, error_task).await;

                Ok(error_msg)
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
    - You can check the patient room using the camerate tool to answer questions about the asked query
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
    tools = [PublishTopicToAnalysis, CameraAnalysisTool],
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
        .subscribe_topic(Topic::<Task>::new("camera_response")) // "camera_response" topic for camera analysis results
        // DO NOT subscribe to "analysis_agent" topic - that's for AnalysisAgent only
        .with_memory(sliding_window_memory)
        .build()
        .await?;

    println!(
        "🔍 DoctorAgent subscribed to topics: ['user_messages', 'analysis_response', 'camera_response']"
    );
    println!("🔍 DoctorAgent processes user messages from 'user_messages' topic (no loops)");
    println!("🔍 DoctorAgent receives analysis results from 'analysis_response' topic");
    println!("🔍 DoctorAgent receives camera analysis results from 'camera_response' topic");

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
                    .publish(
                        &user_messages_topic_clone,
                        Task::new(actual_message.to_string()),
                    )
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

pub async fn run_camera_agent(
    llm: Arc<OpenAI>,
    node_name: String,
    port: u16,
    host_addr: String,
    host: String,
) -> Result<(), Error> {
    println!(
        "📷 Initializing CameraAgent cluster client on port {}",
        port
    );

    let sliding_window_memory = Box::new(SlidingWindowMemory::new(10));
    let camera_topic = Topic::<Task>::new("camera_requests");

    // Create cluster client runtime for CameraAgent - it will connect to dedicated cluster host
    let runtime = ClusterClientRuntime::new(
        "camera_client".to_string(),
        host_addr.clone(),
        node_name,
        "cluster-cookie".to_string(),
        port,
        host,
    );

    println!("📷 Creating CameraAgent instance...");
    let camera_agent = CameraAgent {};

    // Create and initialize agent
    let _agent_instance = AgentBuilder::new(camera_agent)
        .with_llm(llm)
        .runtime(runtime.clone())
        .subscribe_topic(camera_topic.clone())
        .with_memory(sliding_window_memory)
        .build()
        .await?;

    // Create environment and set up event handling
    let mut environment = Environment::new(None);
    let _ = environment.register_runtime(runtime.clone()).await;

    let receiver = environment.take_event_receiver(None).await?;
    let (_dummy_tx, _) = mpsc::unbounded_channel::<String>();

    // Use the regular handle_events function for camera responses
    let (camera_response_tx, _) = mpsc::unbounded_channel::<String>();
    println!("📷 Setting up CameraAgent event handler...");
    handle_events(receiver, camera_response_tx, runtime.clone(), false);

    // Spawn environment runner in background
    let _env_handle = tokio::spawn(async move {
        if let Err(e) = environment.run().await {
            eprintln!("Environment error: {}", e);
        }
    });

    println!(
        "🌐 ClusterClientRuntime will connect to cluster host at {}",
        host_addr
    );

    println!("📷 CameraAgent ready to analyze images for medical queries...");
    println!("📷 CameraAgent subscribed to topic: camera_requests");
    println!("📷 CameraAgent runtime: {:?}", runtime);
    println!("📷 Camera capture methods: ImageSnap (primary), FFmpeg (fallback)");

    // Keep running until Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    println!("📷 Shutting down CameraAgent...");
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
