use iced::{
    button, slider, video, window, Align, Button, Column, Command, Container,
    Length, Settings, Slider, Subscription, Video,
};

#[derive(Clone, Debug)]
enum Message {
    VideoEvent(video::Event),
    SeekPosition(u64),
    PlayPause,
}

enum PlaybackState {
    Playing,
    Paused,
}

struct Application {
    player: video::Player,
    position: u64,
    duration: u64,
    playback_state: PlaybackState,
    slider_state: slider::State,
    play_button_state: button::State,
}

impl iced::Application for Application {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Flags = ();

    fn new(directory: Self::Flags) -> (Self, Command<Self::Message>) {
        let mut player = video::Player::new().unwrap();
        player.set_source("/home/martinsandfuchs/Downloads/sample_video.mp4");
        player.play();
        (
            Self {
                player,
                position: 0,
                duration: 0,
                playback_state: PlaybackState::Playing,
                slider_state: slider::State::new(),
                play_button_state: button::State::new(),
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        String::from("Video player")
    }

    fn update(&mut self, msg: Self::Message) -> Command<Self::Message> {
        match msg {
            Message::PlayPause => {
                self.playback_state = match self.playback_state {
                    PlaybackState::Playing => {
                        self.player.pause();
                        PlaybackState::Paused
                    }
                    PlaybackState::Paused => {
                        self.player.play();
                        PlaybackState::Playing
                    }
                }
            }
            Message::SeekPosition(position) => {
                self.position = position;
                self.player.seek(position);
            }
            Message::VideoEvent(video::Event::SampleReceived(sample)) => {
                if let Some(position) = self.player.position() {
                    self.position = position;
                }
                self.player.set_sample(sample);
            }
            Message::VideoEvent(video::Event::DurationChanged(duration)) => {
                self.duration = duration;
            }
            Message::VideoEvent(_) => {}
        }
        Command::none()
    }

    fn view(&mut self) -> iced::Element<Self::Message> {
        Column::new()
            .push(
                Container::new(
                    Button::new(
                        &mut self.play_button_state,
                        Video::new(&self.player),
                    )
                    .on_press(Message::PlayPause),
                )
                .align_y(Align::Center)
                .width(Length::Fill)
                .height(Length::Fill),
            )
            .push(
                Container::new(Slider::new(
                    &mut self.slider_state,
                    0.0..=self.duration as _,
                    self.position as _,
                    |value| Message::SeekPosition(value as u64),
                ))
                .width(Length::Fill)
                .align_y(Align::End),
            )
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        self.player.events().map(Message::VideoEvent)
    }
}

fn main() {
    <Application as iced::Application>::run(Settings {
        window: window::Settings {
            size: (1600, 1000),
            ..window::Settings::default()
        },
        flags: (),
        ..Settings::default()
    })
}
