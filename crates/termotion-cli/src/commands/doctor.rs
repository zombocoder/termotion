use termotion_encode::ffmpeg::{Ffmpeg, H264_ENCODER, VP9_ENCODER};

/// Exit code for "encoder unavailable" per the documented exit-code table
/// (`7 encoder`); shared with the render pipeline's own encoder failures.
const EXIT_ENCODER: i32 = 7;

/// Checks external dependencies. FFmpeg is optional: PNG sequence output always
/// works, so a missing or incomplete install is reported clearly but must not
/// read as fatal for the whole tool.
pub fn run() -> i32 {
    println!("✓ png sequence output (no external dependencies)");
    println!();

    match Ffmpeg::discover().and_then(|ffmpeg| ffmpeg.probe()) {
        Ok(info) => {
            println!("✓ ffmpeg {}", info.version);
            print_encoder(VP9_ENCODER, info.has_vp9);
            print_encoder(H264_ENCODER, info.has_h264);

            if info.has_vp9 && info.has_h264 {
                0
            } else {
                println!();
                println!(
                    "Some encoders are unavailable. WebM needs {VP9_ENCODER}; MP4 needs {H264_ENCODER}."
                );
                EXIT_ENCODER
            }
        }
        Err(err) => {
            println!("✗ ffmpeg not found");
            println!();
            println!("{err}");
            println!();
            println!("Install FFmpeg, or point termotion at it:");
            println!("  export TERMOTION_FFMPEG=/path/to/ffmpeg");
            println!();
            println!("PNG sequence output works without FFmpeg.");
            EXIT_ENCODER
        }
    }
}

fn print_encoder(name: &str, present: bool) {
    if present {
        println!("✓ {name}");
    } else {
        println!("✗ {name} (not found)");
    }
}
