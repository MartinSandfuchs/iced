//! Display videos in your user interface.
use crate::backend::Backend;
use crate::{Primitive, Renderer};
use iced_native::mouse;
use iced_native::video;
use iced_native::Layout;

impl<B> iced_native::video::Renderer for Renderer<B>
where
    B: Backend,
{
    fn draw(
        &mut self,
        sample: &Option<video::Sample>,
        layout: Layout<'_>,
    ) -> Self::Output {
        let primitive = if let Some(sample) = &sample {
            Primitive::Sample {
                sample: sample.clone(),
                bounds: layout.bounds(),
            }
        } else {
            Primitive::None
        };

        (primitive, mouse::Interaction::default())
    }
}
