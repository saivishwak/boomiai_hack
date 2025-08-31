use iced::widget::{Column, button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Task, Theme};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Message {
    InputChanged(String),
    SendMessage,
    ReceivedDoctorResponse(String),
    Tick,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub content: String,
    pub is_user: bool,
}

pub struct ChatApp {
    messages: Vec<ChatMessage>,
    input_value: String,
    user_sender: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    response_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<String>>>>,
}

impl ChatApp {
    pub fn new(
        user_sender: mpsc::UnboundedSender<String>,
        response_receiver: mpsc::UnboundedReceiver<String>,
    ) -> Self {
        Self {
            messages: vec![ChatMessage {
                content: "Hello! I'm your ECG analysis assistant. I can help you analyze ECG data and provide medical recommendations. How can I assist you today?".to_string(),
                is_user: false,
            }],
            input_value: String::new(),
            user_sender: Arc::new(Mutex::new(Some(user_sender))),
            response_receiver: Arc::new(Mutex::new(Some(response_receiver))),
        }
    }

    pub fn title(&self) -> String {
        String::from("LiquidOS - AI Medical Assistant")
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::InputChanged(value) => {
                self.input_value = value;
            }
            Message::SendMessage => {
                if !self.input_value.trim().is_empty() {
                    let content = self.input_value.clone();

                    // Add user message to chat
                    self.messages.push(ChatMessage {
                        content: content.clone(),
                        is_user: true,
                    });

                    // Send message to doctor agent with USER_SEND prefix to identify actual send events
                    if let Some(sender) = self.user_sender.lock().unwrap().as_ref() {
                        let _ = sender.send(format!("USER_SEND:{}", content));
                    }

                    self.input_value.clear();

                    // Immediately check for responses after sending
                    return Task::done(Message::Tick);
                }
            }
            Message::ReceivedDoctorResponse(response) => {
                self.messages.push(ChatMessage {
                    content: response,
                    is_user: false,
                });
            }
            Message::Tick => {
                // Check for new responses from the doctor agent
                let mut found_messages = false;
                if let Ok(mut guard) = self.response_receiver.lock() {
                    if let Some(receiver) = guard.as_mut() {
                        while let Ok(msg) = receiver.try_recv() {
                            println!("📱 GUI successfully received response: {}", msg);
                            self.messages.push(ChatMessage {
                                content: msg,
                                is_user: false,
                            });
                            found_messages = true;
                        }
                    }
                } else {
                    println!("⚠️ Failed to acquire lock on response receiver");
                }

                // Schedule another check in 1 second
                return Task::perform(
                    async {
                        async_std::task::sleep(std::time::Duration::from_secs(1)).await;
                    },
                    |_| Message::Tick,
                );
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<Message> {
        // Dark theme colors
        let bg_primary = iced::Color::from_rgb(0.1, 0.1, 0.12); // Very dark blue-gray
        let bg_secondary = iced::Color::from_rgb(0.15, 0.15, 0.18); // Slightly lighter
        let bg_input = iced::Color::from_rgb(0.18, 0.18, 0.22); // Input background
        let user_bubble = iced::Color::from_rgb(0.2, 0.4, 0.8); // User message blue
        let ai_bubble = iced::Color::from_rgb(0.25, 0.25, 0.3); // AI message gray
        let text_primary = iced::Color::WHITE;
        let text_secondary = iced::Color::from_rgb(0.9, 0.9, 0.9);
        let accent_green = iced::Color::from_rgb(0.2, 0.8, 0.4);

        let messages_view =
            self.messages
                .iter()
                .fold(Column::new().spacing(12).padding(20), |column, msg| {
                    let message_content = text(&msg.content).size(15).color(text_primary);

                    let message_bubble = if msg.is_user {
                        // User message - right aligned, blue bubble
                        container(message_content)
                            .padding([12, 16])
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(user_bubble)),
                                text_color: Some(text_primary),
                                border: iced::Border {
                                    radius: 16.0.into(),
                                    width: 0.0,
                                    color: iced::Color::TRANSPARENT,
                                },
                                shadow: iced::Shadow {
                                    color: iced::Color::BLACK,
                                    offset: iced::Vector::new(0.0, 2.0),
                                    blur_radius: 8.0,
                                },
                            })
                            .max_width(500)
                    } else {
                        // AI message - left aligned, gray bubble
                        container(message_content)
                            .padding([12, 16])
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(iced::Background::Color(ai_bubble)),
                                text_color: Some(text_primary),
                                border: iced::Border {
                                    radius: 16.0.into(),
                                    width: 0.0,
                                    color: iced::Color::TRANSPARENT,
                                },
                                shadow: iced::Shadow {
                                    color: iced::Color::BLACK,
                                    offset: iced::Vector::new(0.0, 2.0),
                                    blur_radius: 8.0,
                                },
                            })
                            .max_width(500)
                    };

                    let message_row = if msg.is_user {
                        row![]
                            .push(iced::widget::Space::with_width(Length::Fill))
                            .push(message_bubble)
                            .spacing(8)
                    } else {
                        row![]
                            .push(container(text("AI").size(12)).padding([6, 10]).style(
                                move |_theme: &Theme| container::Style {
                                    background: Some(iced::Background::Color(accent_green)),
                                    text_color: Some(text_primary),
                                    border: iced::Border {
                                        radius: 12.0.into(),
                                        width: 0.0,
                                        color: iced::Color::TRANSPARENT,
                                    },
                                    ..Default::default()
                                },
                            ))
                            .push(message_bubble)
                            .push(iced::widget::Space::with_width(Length::Fill))
                            .spacing(8)
                            .align_y(Alignment::Start)
                    };

                    column.push(message_row)
                });

        let chat_area = scrollable(container(messages_view).width(Length::Fill).style(
            move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(bg_primary)),
                ..Default::default()
            },
        ))
        .style(move |_theme: &Theme, _status| scrollable::Style {
            container: container::Style {
                background: Some(iced::Background::Color(bg_primary)),
                ..Default::default()
            },
            vertical_rail: scrollable::Rail {
                background: Some(iced::Background::Color(bg_secondary)),
                border: iced::Border::default(),
                scroller: scrollable::Scroller {
                    color: iced::Color::from_rgb(0.4, 0.4, 0.5),
                    border: iced::Border {
                        radius: 2.0.into(),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                },
            },
            horizontal_rail: scrollable::Rail {
                background: Some(iced::Background::Color(bg_secondary)),
                border: iced::Border::default(),
                scroller: scrollable::Scroller {
                    color: iced::Color::from_rgb(0.4, 0.4, 0.5),
                    border: iced::Border {
                        radius: 2.0.into(),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                },
            },
            gap: None,
        })
        .height(Length::FillPortion(4));

        let input_field = text_input("Type your message here...", &self.input_value)
            .on_input(Message::InputChanged)
            .padding(16)
            .size(16)
            .style(move |_theme: &Theme, _status| text_input::Style {
                background: iced::Background::Color(bg_input),
                border: iced::Border {
                    radius: 12.0.into(),
                    width: 1.0,
                    color: iced::Color::from_rgb(0.3, 0.3, 0.4),
                },
                icon: text_secondary,
                placeholder: text_secondary,
                value: text_primary,
                selection: iced::Color::from_rgb(0.3, 0.5, 0.9),
            });

        let send_button = button(text("Send").size(15).color(text_primary))
            .on_press(Message::SendMessage)
            .padding([14, 20])
            .style(move |_theme: &Theme, status| match status {
                button::Status::Hovered => button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb(
                        0.25, 0.85, 0.45,
                    ))),
                    text_color: text_primary,
                    border: iced::Border {
                        radius: 8.0.into(),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                    shadow: iced::Shadow {
                        color: iced::Color::BLACK,
                        offset: iced::Vector::new(0.0, 4.0),
                        blur_radius: 8.0,
                    },
                },
                button::Status::Pressed => button::Style {
                    background: Some(iced::Background::Color(iced::Color::from_rgb(
                        0.15, 0.75, 0.35,
                    ))),
                    text_color: text_primary,
                    border: iced::Border {
                        radius: 8.0.into(),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                    shadow: iced::Shadow {
                        color: iced::Color::BLACK,
                        offset: iced::Vector::new(0.0, 1.0),
                        blur_radius: 2.0,
                    },
                },
                _ => button::Style {
                    background: Some(iced::Background::Color(accent_green)),
                    text_color: text_primary,
                    border: iced::Border {
                        radius: 8.0.into(),
                        width: 0.0,
                        color: iced::Color::TRANSPARENT,
                    },
                    shadow: iced::Shadow {
                        color: iced::Color::BLACK,
                        offset: iced::Vector::new(0.0, 2.0),
                        blur_radius: 4.0,
                    },
                },
            });

        let input_area = row![input_field, send_button]
            .spacing(12)
            .padding(20)
            .align_y(Alignment::Center);

        let header = container(
            row![
                text("LiquidOS AI").size(20).color(text_primary),
                iced::widget::Space::with_width(Length::Fill),
                text("Online").size(14).color(accent_green)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(20)
        .style(move |_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(bg_secondary)),
            text_color: Some(text_primary),
            border: iced::Border {
                radius: 0.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            shadow: iced::Shadow {
                color: iced::Color::BLACK,
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 4.0,
            },
        });

        let content = column![
            header,
            container(chat_area)
                .height(Length::FillPortion(4))
                .width(Length::Fill)
                .style(move |_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(bg_primary)),
                        ..Default::default()
                    }
                }),
            container(input_area)
                .width(Length::Fill)
                .style(move |_theme: &Theme| {
                    container::Style {
                        background: Some(iced::Background::Color(bg_secondary)),
                        border: iced::Border {
                            radius: 0.0.into(),
                            width: 1.0,
                            color: iced::Color::from_rgb(0.2, 0.2, 0.25),
                        },
                        ..Default::default()
                    }
                })
        ];

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(bg_primary)),
                ..Default::default()
            })
            .into()
    }
}

pub fn run_chat_app(
    user_tx: mpsc::UnboundedSender<String>,
    response_rx: mpsc::UnboundedReceiver<String>,
) -> iced::Result {
    iced::application(ChatApp::title, ChatApp::update, ChatApp::view).run_with(|| {
        let app = ChatApp::new(user_tx, response_rx);
        // Start the polling immediately
        let initial_task = Task::done(Message::Tick);
        (app, initial_task)
    })
}
