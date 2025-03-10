use crate::frame::object::Object;
use crate::frame::processor::Processor;
use crate::predictor::Controller;
use std::{cell::RefCell, rc::Rc, thread, time::Duration};
use wayland_client::{
    protocol::wl_output::WlOutput, Display as WaylandDisplay, EventQueue, GlobalManager, Main,
};
use wayland_protocols::wlr::unstable::export_dmabuf::v1::client::{
    zwlr_export_dmabuf_frame_v1::{CancelReason, Event},
    zwlr_export_dmabuf_manager_v1::ZwlrExportDmabufManagerV1,
};

const DELAY_SUCCESS: Duration = Duration::from_millis(100);
const DELAY_FAILURE: Duration = Duration::from_millis(1000);

#[derive(Clone)]
pub struct Capturer {
    event_queue: Rc<RefCell<EventQueue>>,
    output: Main<WlOutput>,
    dmabuf_manager: Main<ZwlrExportDmabufManagerV1>,
    processor: Rc<dyn Processor>,
}

impl super::Capturer for Capturer {
    fn run(&self, controller: Controller) {
        Rc::new(self.clone()).capture_frame(Rc::new(RefCell::new(controller)));

        loop {
            self.event_queue
                .borrow_mut()
                .dispatch(&mut (), |_, _, _| {})
                .unwrap();
        }
    }
}

impl Capturer {
    pub fn new(processor: Box<dyn Processor>) -> Self {
        let display = WaylandDisplay::connect_to_env().unwrap();
        let mut event_queue = display.create_event_queue();
        let attached_display = display.attach(event_queue.token());
        let globals = GlobalManager::new(&attached_display);

        event_queue.sync_roundtrip(&mut (), |_, _, _| {}).unwrap();

        let output = globals
            .instantiate_exact::<WlOutput>(1)
            .expect("unable to init wayland output");

        let dmabuf_manager = globals
            .instantiate_exact::<ZwlrExportDmabufManagerV1>(1)
            .expect("unable to init export_dmabuf_manager");

        Self {
            event_queue: Rc::new(RefCell::new(event_queue)),
            output,
            dmabuf_manager,
            processor: processor.into(),
        }
    }

    fn capture_frame(self: Rc<Self>, controller: Rc<RefCell<Controller>>) {
        let mut frame = Object::default();

        self.dmabuf_manager
            .capture_output(0, &self.output)
            .quick_assign(move |data, event, _| match event {
                Event::Frame {
                    width,
                    height,
                    num_objects,
                    ..
                } => {
                    frame.set_metadata(width, height, num_objects);
                }

                Event::Object {
                    index, fd, size, ..
                } => {
                    frame.set_object(index, fd, size);
                }

                Event::Ready { .. } => {
                    let luma = self
                        .processor
                        .luma_percent(&frame)
                        .expect("Unable to compute luma percent");

                    controller.borrow_mut().adjust(Some(luma));

                    data.destroy();

                    thread::sleep(DELAY_SUCCESS);
                    self.clone().capture_frame(controller.clone());
                }

                Event::Cancel { reason } => {
                    data.destroy();

                    if reason == CancelReason::Permanent {
                        panic!("Frame was cancelled due to a permanent error. If you just disconnected screen, this is not implemented yet.");
                    } else {
                        eprintln!("Frame was cancelled due to a temporary error, will try again.");
                        thread::sleep(DELAY_FAILURE);
                        self.clone().capture_frame(controller.clone());
                    }
                }

                _ => unreachable!(),
            });
    }
}
