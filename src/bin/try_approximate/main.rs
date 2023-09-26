mod cli;

use clap::Parser;

use laser_precision_adjust::predict::{aproximate_exp, find_min, Fragment, NORMAL_T};
use laser_precision_adjust::IDataPoint;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
    let cmd = cli::Cli::parse();

    let file = std::fs::File::open(cmd.json_file)?;

    let data = serde_json::from_reader::<_, Vec<Vec<Fragment<f64>>>>(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for (ch_num, shots) in data.iter().enumerate() {
        println!("\nChannel #{}:", ch_num);
        for shot in shots {
            let data = shot.points();

            let x = data.iter().map(|p| p.x()).collect::<Vec<_>>();
            let y = data.iter().map(|p| p.y()).collect::<Vec<_>>();
            if let Some((f_min_index, min_f)) = find_min(&y) {
                let fz = y[f_min_index..]
                    .iter()
                    .map(move |f| *f - min_f)
                    .collect::<Vec<_>>();
                let t_zero = x[f_min_index];
                let tz = x[f_min_index..]
                    .iter()
                    .map(move |t| (*t - t_zero) / NORMAL_T)
                    .collect::<Vec<_>>();

                if let Ok(coeffs) = aproximate_exp(tz, &fz) {
                    let orig_coeffs = shot.aprox_coeffs();
                    println!(
                        "Coeffs:;{};{};;{};{}",
                        coeffs.0, coeffs.1, orig_coeffs.0, orig_coeffs.1
                    );
                    let new_fragment =
                        Fragment::new(shot.start_timestamp() as u128, &data, coeffs, f_min_index);
                    let orig_model_data = shot.evaluate();
                    let new_data = new_fragment.evaluate();
                    for (((i, p), pm), pnm) in
                        data.iter().enumerate().zip(orig_model_data).zip(new_data)
                    {
                        println!("{};{};{};{};{}", i, p.x(), p.y(), pm.y(), pnm.y());
                    }
                }
            }
        }
        println!();
    }

    Ok(())
}
