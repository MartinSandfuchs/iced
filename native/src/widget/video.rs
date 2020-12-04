//! Display video in your application with GStreamer.

mod player;

use crate::{layout, Element, Hasher, Layout, Length, Point, Size, Widget};
use std::hash::Hash;

pub use player::{Event, Player, Sample};

/// A frame that displays a video while keeping aspect ratio.
#[derive(Clone, Debug)]
pub struct Video {
    sample: Option<player::Sample>,
    width: Length,
    height: Length,
}

impl Video {
    /// Create a new [`Video`] widget which displays the current frame of the provided [`Player`]
    ///
    /// [`Video`]: struct.Video.html
    /// [`Player`]: struct.Player.html
    pub fn new(player: &player::Player) -> Self {
        Self {
            sample: player.sample.clone(),
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    /// Sets the width of the [`Video`] boundaries.
    ///
    /// [`Video`]: struct.Video.html
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Sets the height of the [`Video`] boundaries.
    ///
    /// [`Video`]: struct.Video.html
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl<Message, Renderer> Widget<Message, Renderer> for Video
where
    Renderer: self::Renderer,
{
    fn width(&self) -> Length {
        self.width
    }

    fn height(&self) -> Length {
        self.height
    }

    fn layout(
        &self,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let mut size = limits
            .width(self.width)
            .height(self.height)
            .resolve(Size::INFINITY);
        if let Some(sample) = &self.sample {
            let (width, height) = (sample.width as f32, sample.height as f32);
            let aspect = width / height;

            let viewport_aspect = size.width / size.height;

            if viewport_aspect > aspect {
                size.width = size.height * aspect;
            } else {
                size.height = size.width / aspect;
            }
        }

        layout::Node::new(size)
    }

    fn hash_layout(&self, state: &mut Hasher) {
        if let Some(sample) = &self.sample {
            sample.stream_id.hash(state);
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        _defaults: &Renderer::Defaults,
        layout: Layout<'_>,
        _cursor_position: Point,
        _viewport: &iced_core::Rectangle,
    ) -> Renderer::Output {
        renderer.draw(&self.sample, layout)
    }
}

/// The renderer of a [`Video`].
///
/// Your [renderer] will need to implement this trait before being able to use
/// an [`Video`] in your user interface.
///
/// [`Video`]: struct.Video.html
/// [renderer]: ../../renderer/index.html
pub trait Renderer: crate::Renderer {
    /// Draws a [`Video`].
    ///
    /// [`Video`]: struct.Video.html
    fn draw(
        &mut self,
        sample: &Option<player::Sample>,
        layout: Layout<'_>,
    ) -> Self::Output;
}

impl<'a, Message, Renderer> From<Video> for Element<'a, Message, Renderer>
where
    Renderer: self::Renderer,
{
    fn from(video: Video) -> Element<'a, Message, Renderer> {
        Element::new(video)
    }
}
