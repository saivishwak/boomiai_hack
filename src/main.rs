use iced::widget::Text;
use iced::{Element, Task, Theme};

#[derive(Debug, Clone)]
enum Message {}

struct App;

impl App {
    fn new() -> (Self, Task<Message>) {
        (Self, Task::none())
    }

    fn title(&self) -> String {
        String::from("Simple Text GUI")
    }

    fn update(&mut self, _message: Message) -> Task<Message> {
        Task::none()
    }

    fn view(&self) -> Element<Message> {
        Text::new("Hello, Iced GUI!")
            .size(50)
            .into()
    }
}

fn main() -> iced::Result {
    iced::application("Simple Text GUI", App::update, App::view)
        .theme(|_| Theme::Dark)
        .run_with(App::new)
}
