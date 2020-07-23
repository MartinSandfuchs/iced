use crate::futures::Stream;
use crate::Subscription;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::AppSink;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll, Waker};

/// A sample from a video stream.
#[derive(Clone, Debug)]
pub struct Sample {
    /// The GStreamer sample.
    pub gst_sample: gstreamer::Sample,
    /// Width of the sample.
    pub width: i32,
    /// Height of the sample.
    pub height: i32,
    /// Id of the stream from which the sample originated.
    pub stream_id: u64,
    /// Id of the sample.
    pub sample_id: u64,
    /// Whether the sample originates from a preroll event.
    pub from_preroll: bool,
}

impl PartialEq for Sample {
    fn eq(&self, other: &Self) -> bool {
        self.stream_id == other.stream_id && self.sample_id == other.sample_id
    }
}

/// An event created by the [`Player`].
///
/// [`Player`]: struct.Player.html
#[derive(Debug, Clone)]
pub enum Event {
    /// A new sample was created.
    SampleReceived(Sample),
    /// The resolution of the stream has changed.
    ResolutionChanged {
        /// The new width.
        width: i32,
        /// The new height.
        height: i32,
    },
    /// The duration of the stream has changed.
    DurationChanged(u64),
}

/// Play videos with GStreamer.
#[derive(Debug, Clone)]
pub struct Player {
    playbin: gst::Element,
    app_sink: AppSink,
    event_stream: EventStream,
    pub(super) sample: Option<Sample>,
}

impl Player {
    /// Create a new video player. Returns None if the required GStreamer modules could not be
    /// loaded. This is usually caused by missing GStreamer plugins.
    pub fn new() -> Option<Self> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Initialize gstreamer
        gst::init().ok()?;

        let sink = gst::ElementFactory::make("appsink", None).ok()?;
        let app_sink = sink
            .dynamic_cast::<AppSink>()
            .expect("Sink element is expected to be an appsink");
        app_sink.set_caps(Some(&gst::Caps::new_simple(
            "video/x-raw",
            &[
                ("format", &"BGRA"),
                // ("width", &width), // This could be used to force a specific resolution
                ("pixel-aspect-ratio", &gst::Fraction::new(1, 1)),
            ],
        )));
        app_sink
            .set_property("enable-last-sample", &false.to_value())
            .ok()?;
        app_sink.set_max_buffers(1);
        app_sink.set_emit_signals(false);

        let playbin = gst::ElementFactory::make("playbin", None).ok()?;
        playbin.set_property("video_sink", &app_sink).ok()?;

        // Construct the event stream
        let mut hasher = DefaultHasher::new();
        app_sink.hash(&mut hasher);
        let stream_id = hasher.finish();
        let event_stream =
            EventStream::new(stream_id, app_sink.clone(), playbin.clone());

        Some(Self {
            playbin,
            app_sink,
            event_stream,
            sample: None,
        })
    }

    /// Set the sample to be displayed by the [`Player`]. This is required to change the contents
    /// of any [`Video`] widgets using this [`Player`]. Use [`Player::events`] to create a
    /// [`Subscription`] to listen to incoming video samples.
    ///
    /// [`Player`]: struct.Player.html
    /// [`Video`]: struct.Video.html
    /// [`Player::events`]: struct.Player.html#method.events
    /// [`Subscription`]: ../../subscription/type.Subscription.html
    pub fn set_sample(&mut self, sample: Sample) {
        self.sample = Some(sample);
    }

    /// Set the source of the stream.
    pub fn set_source(&mut self, path: &str) {
        let mut uri = String::from("file://");
        uri.push_str(path);

        let set_source = || {
            let _ = self.playbin.set_state(gst::State::Ready).ok()?;
            let _ = self.playbin.set_property("uri", &uri).ok()?;
            let _ = self.playbin.set_state(gst::State::Paused).ok()?;
            Some(())
        };
        let _ = set_source();
    }

    /// Seek to a specific `position` in the stream, where `position` is given in seconds.
    pub fn seek(&mut self, position: u64) {
        let _ = self.playbin.seek_simple(
            // gst::SeekFlags::from_bits(1).unwrap(),
            gst::SeekFlags::FLUSH,
            gst::ClockTime::from_seconds(position),
        );
    }

    /// Start playback.
    pub fn play(&mut self) {
        let _ = self.playbin.set_state(gst::State::Playing);
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        let _ = self.playbin.set_state(gst::State::Paused);
    }

    /// Query the video position.
    pub fn position(&self) -> Option<u64> {
        let position = self.playbin.query_position::<gst::ClockTime>()?;
        position.seconds()
    }

    /// Set the volume in the range [0, 1].
    pub fn set_volume(&mut self, volume: f64) {
        let _ = self.playbin.set_property("volume", &volume);
    }

    /// Create a [`Subscription`] for the events of this [`Player`]. This is required to update the currently
    /// displayed frame of any [`Video`] widgets using this [`Player`]. New frames can be set using
    /// [`Player::set_sample`].
    ///
    /// [`Video`]: struct.Video.html
    /// [`Player`]: struct.Player.html
    /// [`Player::set_sample`]: struct.Player.html#method.set_sample
    /// [`Subscription`]: ../../subscription/type.Subscription.html
    pub fn events(&self) -> Subscription<Event> {
        Subscription::from_recipe(self.event_stream.clone())
    }
}

#[derive(Debug, Clone)]
struct EventStreamShared {
    waker: Option<Waker>,
    event_queue: Vec<Event>,
    stream_id: u64,
    sample_id: u64,
}

#[derive(Debug, Clone)]
struct EventStream {
    shared: Arc<RwLock<EventStreamShared>>,
}

impl EventStream {
    fn new(stream_id: u64, app_sink: AppSink, playbin: gst::Element) -> Self {
        let shared = Arc::new(RwLock::new(EventStreamShared {
            waker: None,
            event_queue: Vec::new(),
            stream_id,
            sample_id: 0,
        }));
        // Listen for changes to the stream's duration and resolution
        let _ = playbin.connect("video-tags-changed", false, {
            let shared = Arc::clone(&shared);
            let playbin = playbin.clone();
            move |_| {
                let mut shared = shared.write().unwrap();
                if let Some(waker) = shared.waker.take() {
                    // This is the resolution of the media, rather than the sample
                    let resolution = || {
                        let pad = playbin.emit("get-video-pad", &[&0]).ok()?;
                        let pad: gst::Pad = pad?.get().ok()??;
                        let caps = pad.get_current_caps()?;
                        let structure = caps.get_structure(0)?;
                        let width: i32 = structure.get("width").ok()??;
                        let height: i32 = structure.get("height").ok()??;
                        Some((width, height))
                    };
                    if let Some((width, height)) = resolution() {
                        let event = Event::ResolutionChanged { width, height };
                        shared.event_queue.push(event);
                    }

                    let duration = || {
                        let t = playbin.query_duration::<gst::ClockTime>()?;
                        t.seconds()
                    };
                    if let Some(t) = duration() {
                        shared.event_queue.push(Event::DurationChanged(t));
                    }
                    waker.wake();
                }
                None
            }
        });
        // Helper function to extract the resolution of a sample
        fn sample_resolution(sample: &gstreamer::Sample) -> Option<(i32, i32)> {
            let structure = sample.get_caps()?.get_structure(0)?;
            let width: i32 = structure.get("width").ok()??;
            let height: i32 = structure.get("height").ok()??;
            Some((width, height))
        };
        let timeout = gst::ClockTime::from_mseconds(0);
        // Listen for new samples
        app_sink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample({
                    let shared = Arc::clone(&shared);
                    let app_sink = app_sink.clone();
                    move |_| {
                        let shared = &mut *shared.write().unwrap();
                        let build_sample = || {
                            let gst_sample =
                                app_sink.try_pull_sample(timeout)?;
                            let (width, height) =
                                sample_resolution(&gst_sample)?;
                            Some(Sample {
                                gst_sample,
                                width,
                                height,
                                from_preroll: false,
                                stream_id: shared.stream_id,
                                sample_id: shared.sample_id,
                            })
                        };
                        if let Some(sample) = build_sample() {
                            if let Some(waker) = shared.waker.take() {
                                shared.sample_id += 1;
                                let event = Event::SampleReceived(sample);
                                shared.event_queue.push(event);
                                waker.wake();
                            }
                        }
                        Ok(gst::FlowSuccess::Ok)
                    }
                })
                .new_preroll({
                    let shared = Arc::clone(&shared);
                    let app_sink = app_sink.clone();
                    move |_| {
                        let shared = &mut *shared.write().unwrap();
                        let build_sample = || {
                            let gst_sample =
                                app_sink.try_pull_preroll(timeout)?;
                            let (width, height) =
                                sample_resolution(&gst_sample)?;
                            Some(Sample {
                                gst_sample,
                                width,
                                height,
                                from_preroll: true,
                                stream_id: shared.stream_id,
                                sample_id: shared.sample_id,
                            })
                        };
                        if let Some(sample) = build_sample() {
                            if let Some(waker) = shared.waker.take() {
                                shared.sample_id += 1;
                                let event = Event::SampleReceived(sample);
                                shared.event_queue.push(event);
                                waker.wake();
                            }
                        }
                        Ok(gst::FlowSuccess::Ok)
                    }
                })
                .eos({
                    let shared = Arc::clone(&shared);
                    move |_| {
                        let shared = &mut *shared.write().unwrap();
                        if let Some(waker) = shared.waker.take() {
                            waker.wake();
                        }
                    }
                })
                .build(),
        );

        Self { shared }
    }
}

impl Stream for EventStream {
    type Item = Event;

    fn poll_next(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut shared = self.shared.write().unwrap();
        let _ = shared.waker.replace(context.waker().to_owned());
        if let Some(event) = shared.event_queue.pop() {
            context.waker().wake_by_ref();
            Poll::Ready(Some(event))
        } else {
            Poll::Pending
        }
    }
}

impl<H, I> crate::subscription::Recipe<H, I> for EventStream
where
    H: std::hash::Hasher,
{
    type Output = Event;

    fn hash(&self, state: &mut H) {
        use std::hash::Hash;

        std::any::TypeId::of::<Self>().hash(state);
        Arc::into_raw(self.shared.clone()).hash(state);
    }

    fn stream(
        self: Box<Self>,
        _input: iced_futures::BoxStream<I>,
    ) -> iced_futures::BoxStream<Self::Output> {
        Box::pin(self)
    }
}
