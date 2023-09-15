use std::vec;

use clap::Parser;
use laser_precision_adjust::box_plot::BoxPlot;
use num_traits::{Float, FromPrimitive};
use serde::Deserialize;
use smoothspline::{DataPoint, IDataPoint, SmoothSpline, IValue, IDifferentiable, I2Differentiable};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};

mod cli;

#[derive(Deserialize, Debug, Clone, Copy)]
struct Point {
    channel: u32,
    f: f64,
}

const SERIE_SIZE: usize = 100;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    // parse the CLI arguments
    let args = cli::Cli::parse();

    eprintln!("Smooth factor: {:?}", args.smooth);
    eprintln!("Filename: {:?}", args.filename);
    eprintln!("Serie: {:?}", args.serie);

    let mut series = vec![];
    let mut current_serie = vec![];
    let mut prev_channel = None;

    // read the file
    let file = File::open(args.filename).await?;

    let reader = BufReader::new(file);

    // get iterator over lines
    let mut lines = reader.lines();

    // this has to be used instead of a for loop, since lines isn't a
    // normal iterator, but a Lines struct, the next element of which
    // can be obtained using the next_line function.
    let mut _line_num = 0u32;
    while let Some(line) = lines.next_line().await.expect("Failed to read file") {
        // parse the line as json
        match serde_json::from_str::<Point>(&line) {
            Ok(point) => {
                _line_num += 1;
                let channel_name = point.channel;
                if (prev_channel.is_some() && prev_channel != Some(channel_name))
                    || current_serie.len() == SERIE_SIZE
                {
                    if current_serie.len() == SERIE_SIZE {
                        series.push(current_serie);
                    }
                    current_serie = vec![point.f];
                    prev_channel.replace(channel_name);
                } else if prev_channel.is_none() {
                    current_serie = vec![point.f];
                    prev_channel.replace(channel_name);
                } else {
                    current_serie.push(point.f);
                }
            }
            Err(_) => {}
        }
    }

    let serie = series.get(args.serie).unwrap();
    let serie = serie
        .iter()
        .enumerate()
        .map(|(i, f)| DataPoint::new(i as f64, *f))
        .collect::<Vec<_>>();

    let (smooth_serie, new_smooth_serie) = filter_serie(&serie, args.smooth.unwrap_or_default());

    for (o, (s, ns)) in serie.iter().zip(smooth_serie.iter().zip(new_smooth_serie)) {
        println!("{};{};{};{};{}", o.y(), s, ns.0, ns.1, ns.2);
    }

    Ok(())
}

fn filter_serie<T, U>(data: &[U], factor: T) -> (Vec<T>, Vec<(T, T, T)>)
where
    T: Float + FromPrimitive,
    U: smoothspline::IDataPoint<T> + Clone,
{
    let mut spline = SmoothSpline::<_, _, smoothspline::SplineFragment<_>>::new(data);
    spline.update(factor);

    let mut smooth_serie = vec![];
    for i in 0..data.len() {
        let y = spline.y(T::from_usize(i).unwrap());
        if y.is_nan() {
            smooth_serie.push(data[i].y());
        } else {
            smooth_serie.push(y);
        }
    }

    let diffs = smooth_serie
        .iter()
        .zip(data.iter())
        .map(|(s, d)| (*s - d.y()).abs())
        .collect::<Vec<_>>();

    let box_plot = BoxPlot::new(&diffs);

    let new_points = data
        .iter()
        .zip(diffs)
        .filter_map(|(p, d)| {
            if d > box_plot.lower_bound() && d < box_plot.upper_bound() {
                Some(p.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let mut new_spline = SmoothSpline::<_, _, smoothspline::SplineFragment<_>>::new(&new_points);
    new_spline.update(factor);

    let new_smooth_serie = data.iter().map(|p| {
        let x = p.x();
        let fragment = new_spline.find_fragment(x).unwrap();
        (fragment.y(x), fragment.dy(x), fragment.d2y(x))
    }).collect::<Vec<_>>();

    (smooth_serie, new_smooth_serie)
}
