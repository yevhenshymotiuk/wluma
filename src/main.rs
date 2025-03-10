use std::sync::mpsc;

mod als;
mod brightness;
mod config;
mod device_file;
mod frame;
mod predictor;

fn main() {
    let config = match config::Config::load() {
        Ok(config) => config,
        Err(err) => panic!("Unable to load config: {}", err),
    };
    println!("Using config: {:?}", config);

    let als: Box<dyn als::Als> = match config.als {
        config::Als::Iio { path, thresholds } => Box::new(
            als::iio::Als::new(&path, thresholds).expect("Unable to initialize ALS IIO sensor"),
        ),
        config::Als::Time { ref hour_to_lux } => Box::new(als::time::Als::new(hour_to_lux)),
        config::Als::None => Box::new(als::none::Als::default()),
    };

    let frame_processor: Box<dyn frame::processor::Processor> = match config.frame.processor {
        config::Processor::Vulkan => Box::new(
            frame::processor::vulkan::Processor::new().expect("Unable to initialize Vulkan"),
        ),
    };

    let frame_capturer: Box<dyn frame::capturer::Capturer> = match config.frame.capturer {
        config::Capturer::Wlroots => {
            Box::new(frame::capturer::wlroots::Capturer::new(frame_processor))
        }
        config::Capturer::None => Box::new(frame::capturer::none::Capturer::default()),
    };

    let (user_tx, user_rx) = mpsc::channel();
    let (prediction_tx, prediction_rx) = mpsc::channel();

    let config_outputs = config.output.clone();

    std::thread::spawn(move || {
        let brightness = match config_outputs.iter().next().unwrap().1 {
            config::Output::Backlight(cfg) => Box::new(
                brightness::Backlight::new(&cfg.path)
                    .expect("Unable to initialize output backlight"),
            ),
            _ => unimplemented!("Only backlight-controlled outputs are supported"),
        };

        let mut brightness_controller =
            brightness::Controller::new(brightness, user_tx, prediction_rx);

        brightness_controller.run().unwrap();
    });

    let controller = predictor::Controller::new(prediction_tx, user_rx, als, true);

    println!("Continue adjusting brightness and wluma will learn your preference over time.");
    frame_capturer.run(controller);
}
