#![allow(dead_code)]

mod circuit;
mod channels;
mod language;
mod tracking;

use crate::channels::ipc::{retreive_messages, IpcWriter};
use crate::circuit::Leaf;
use crate::circuit::ReactiveCircuit;
use crate::channels::clustering::frequency_adaptation;

use circuit::leaf::{activate_channel, Foliage};
use itertools::Itertools;
use rand::seq::SliceRandom;
use rand_distr::{Distribution, SkewNormal};
use rayon::prelude::{IntoParallelRefMutIterator, ParallelIterator};

use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::vec;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;


pub fn power_set<T: Clone>(leafs: &[T]) -> Vec<Vec<T>> {
    let mut power_set = Vec::new();
    for i in 0..leafs.len() + 1 {
        for set in leafs.iter().cloned().combinations(i) {
            power_set.push(set);
        }
    }
    power_set
}

pub fn random_set(number_leafs: u16, number_sets: usize) -> Vec<Vec<u16>> {
    let mut random_set = Vec::new();

    let mut rng = rand::thread_rng();
    for _ in 0..number_sets {
        random_set.push(
            (0..number_leafs)
                .collect_vec()
                .choose_multiple(&mut rng, number_leafs as usize / 2)
                .cloned()
                .collect(),
        );
    }
    random_set
}

fn randomized_rc(number_leafs: u16, number_models: usize) -> (ReactiveCircuit, Foliage) {
    // let mut rc = RC::new();
    let mut foliage = vec![];
    let mut rc = ReactiveCircuit::new();
    for i in 0..number_leafs {
        foliage.push(Leaf::new(&0.0, &0.0, &i.to_string()));
        // rc.grow(0.0, &i.to_string());
    }
    let foliage = Arc::new(Mutex::new(foliage));

    // let combinations = power_set(&(0..number_leafs).collect_vec());
    let combinations = random_set(number_leafs, number_models);
    for combination in combinations {
        rc = rc + combination;
    }

    rc.set_dependencies(&mut foliage.lock().unwrap());
    (rc, foliage)
}

// fn frequency_adaptation(rc: &mut ReactiveCircuit, foliage: &mut Foliage) {
//     let mut indexed_frequencies_pairs: Vec<(usize, Leaf)> = vec![];
//     for (i, leaf) in foliage.lock().unwrap().iter().enumerate() {
//         let position = indexed_frequencies_pairs.binary_search_by(|pair| {
//             pair.1
//                 .get_frequency()
//                 .partial_cmp(&leaf.get_frequency())
//                 .unwrap()
//         });
//         match position {
//             Ok(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
//             Err(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
//         }
//     }

//     let frequencies: Vec<f64> = indexed_frequencies_pairs
//         .iter()
//         .map(|(_, leaf)| leaf.get_frequency())
//         .collect();

//     // let dataset = Dataset::new(
//     //     Array2::from_shape_vec((frequencies.len(), 1), frequencies).unwrap(),
//     //     array![0.0],
//     // );

//     // let clusters = Dbscan::params(2).tolerance(0.02).transform(dataset);

//     // let mut cluster_counter = 0;
//     // let mut previous_cluster = 0;
//     let mut cluster_steps = vec![];

//     // let mut foliage_guard = rc.foliage.lock().unwrap();
//     // for index in 0..clusters.records.shape()[0] {
//     //     match clusters.targets[index] {
//     //         Some(cluster) => {
//     //             if cluster == previous_cluster {
//     //                 cluster_steps.push(
//     //                     foliage_guard[indexed_frequencies_pairs[index].0]
//     //                         .set_cluster(&cluster_counter),
//     //                 );
//     //             } else {
//     //                 previous_cluster = cluster;
//     //                 cluster_counter += 1;
//     //                 cluster_steps.push(
//     //                     foliage_guard[indexed_frequencies_pairs[index].0]
//     //                         .set_cluster(&cluster_counter),
//     //                 );
//     //             }
//     //         }
//     //         None => {
//     //             previous_cluster = usize::MAX;
//     //             cluster_counter += 1;
//     //             cluster_steps.push(
//     //                 foliage_guard[indexed_frequencies_pairs[index].0].set_cluster(&cluster_counter),
//     //             );
//     //         }
//     //     }
//     // }
//     // drop(foliage_guard);

//     let mut foliage_guard = foliage.lock().unwrap();
//     let boundaries = vec![0.01, 0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 10.0, 100.0];
//     for (index, frequency) in frequencies.iter().enumerate() {
//         for (cluster, boundary) in boundaries.iter().enumerate() {
//             if *frequency <= *boundary {
//                 cluster_steps.push(
//                     foliage_guard[indexed_frequencies_pairs[index].0]
//                         .set_cluster(&(cluster as i32)),
//                 );
//                 break;
//             }
//         }
//     }
//     drop(foliage_guard);

//     if cluster_steps.iter().all(|step| *step == 0) {
//         return;
//     }

//     let min_cluster = cluster_steps.iter().min().unwrap().clone();
//     for step in &mut cluster_steps {
//         *step -= min_cluster;
//     }

//     // println!("Clusters {:?}", clusters.targets);
//     println!("Cluster steps {:?}", cluster_steps);

//     for (index, cluster_step) in cluster_steps.iter().enumerate() {
//         if cluster_step != &0 {
//             if cluster_step > &0 {
//                 rc.drop(
//                     indexed_frequencies_pairs[index].0 as u16,
//                     // *cluster_step as usize - 1,
//                 );
//             } else {
//                 panic!("Not implemented!");
//                 // rc.collect(
//                 //     indexed_frequencies_pairs[index].0 as u16,
//                 //     -*cluster_step as usize - 1,
//                 // );
//             }
//         }
//         if index % 100 == 0 {
//             println!("Done {index}");
//         }
//     }
//     rc.clear_dependencies(&mut foliage.lock().unwrap());
//     rc.set_dependencies(&mut foliage.lock().unwrap());
//     // rc.empty_scope();
// }

// pub fn message_loop(foliage: Foliage, ok: AtomicBool) -> JoinHandle<Result<(), rclrs::RclrsError>> {
//     std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
//         while ok.load(Acquire) {
//             let foliage_guard = foliage.lock().unwrap();
//             retreive_messages();
//             drop(foliage_guard);
//         }
//     })
// }

fn randomized_study() {
    let number_leafs = 2000;
    let number_models = 100000;
    let number_inferences = 1000;
    let scale = 1.0;
    let shape = 0.0;
    let boundaries = vec![0.01, 0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 10.0, 100.0];

    for location in vec![2.0] {
        println!("Building randomized RC.");
        let (mut rc, mut foliage) = randomized_rc(number_leafs, number_models);

        println!("Activate randomized IPC.");
        let mut true_frequencies = sample_frequencies(location, scale, shape, number_leafs as usize);
        for (index, frequency) in true_frequencies.iter_mut().enumerate() {
            let channel = format!("leaf_{}", foliage.lock().unwrap()[index as usize].name);
            activate_channel(foliage.clone(), index as usize, &channel, &false);

            // let mut rng = rand::thread_rng();
            if *frequency < 0.001 {
                *frequency = 0.001;
            }

            let new_publisher = IpcWriter::new(
                &foliage.lock().unwrap()[index as usize]
                    .ipc_channel
                    .as_ref()
                    .unwrap()
                    .topic,
                *frequency,
                1.0 //rng.gen_range(0.1..1.0),
            );
            foliage.lock().unwrap()[index].set_frequency(&frequency);
            new_publisher.unwrap().start();
        }
        let mut sorted_frequencies = true_frequencies.clone();
        sorted_frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("F = {:?}", sorted_frequencies);

        let mut inference_times = vec![];
        let mut values = vec![];
        let mut adaptation_times = vec![];

        inference_times.reserve(number_inferences * 2);
        values.reserve(number_inferences * 2);

        println!("Loop original for {} steps.", number_inferences);
        while values.len() < number_inferences {
            retreive_messages();

            let foliage = foliage.lock().unwrap();
            let before = Instant::now();
            let value = rc.value(&foliage);
            let elapsed = before.elapsed().as_secs_f64();
            drop(foliage);

            inference_times.push(elapsed);
            values.push(value);

            if values.len() % 100 == 0 {
                println!("Done {}", values.len());
            }
        }

        let before = Instant::now();
        frequency_adaptation(&mut rc, &mut foliage, &boundaries);
        adaptation_times.push(before.elapsed().as_secs_f64());
        println!(
            "#Adaptations in {}s",
            adaptation_times[adaptation_times.len() - 1]
        );

        let mut deploy = rc.deploy();
        deploy.reverse();
        println!("Loop deployed for {} steps.", number_inferences);
        while values.len() < 2 * number_inferences {
            retreive_messages();

            let foliage_guard = foliage.lock().unwrap();
            let before = Instant::now();
            deploy.par_iter_mut().for_each(|rc| {
                rc.value(&foliage_guard);
            });
            // deploy.iter_mut().for_each(|rc| { rc.value(&foliage_guard); });
            let value = rc.value(&foliage_guard);
            let elapsed = before.elapsed().as_secs_f64();
            drop(foliage_guard);

            values.push(value);
            if values.len() % 100 == 0 {
                println!("Done {}", values.len());
            }

            inference_times.push(elapsed);
        }

        println!("Export results.");
        let path = Path::new("output/inference_times.csv");
        if !path.exists() {
            let mut file = File::create(path).expect("Unable to create file");
            file.write_all("Time,Runtime,Leafs,Shape,Location,Value\n".as_bytes()).expect("Unable to write data");
        }

        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        let mut csv_text = "".to_string();
        for i in 0..inference_times.len() {
            csv_text.push_str(&format!(
                "{i},{},{},{shape},{location},{}\n",
                inference_times[i],
                number_leafs as usize / 2 * number_models as usize,
                values[i]
            ));
        }
        file.write_all(csv_text.as_bytes())
            .expect("Unable to write data");

        export_frequencies(Path::new("output/frequencies.csv"), location, scale, shape, 100000);
    }
}

fn sample_frequencies(location: f64, scale: f64, shape: f64, number_samples: usize) -> Vec<f64> {
    let distribution = SkewNormal::new(location, scale, shape).unwrap();
    let mut rng = rand::thread_rng();

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        frequencies.push(distribution.sample(&mut rng));
    }

    frequencies
}

fn export_frequencies(path: &Path, location: f64, scale: f64, shape: f64, number_samples: usize) {
    let frequencies = sample_frequencies(location, scale, shape, number_samples);

    if !path.exists() {
        let mut file = File::create(path).expect("Unable to create file");
        file.write_all("Frequency,Location,Scale,Shape\n".as_bytes())
            .expect("Unable to write data");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let mut csv_text = "".to_string();
    for frequency in frequencies {
        csv_text.push_str(&format!("{frequency},{location},{scale},{shape}\n"));
    }

    file.write_all(csv_text.as_bytes())
        .expect("Unable to write data");
}

fn main() -> std::io::Result<()> {
    randomized_study();

    // let args = Args::parse();

    // let model = read_to_string(args.source).unwrap();
    // let mut resin = compile(model);

    // for leaf in resin.leafs.values() {
    //     match &leaf.lock().unwrap().ipc_channel {
    //         Some(channel) => {
    //             use rand::Rng;
    //             let mut rng = rand::thread_rng();
    //             let new_publisher =
    //                 RandomizedIpcChannel::new(&channel.topic, rng.gen_range(0.1..50.0));
    //             new_publisher.unwrap().start();
    //         }
    //         None => (),
    //     }
    // }

    // let clock = SystemTime::now();
    // let mut operations = vec![];
    // let mut times = vec![];
    // loop {
    //     retreive_messages();
    //     frequency_adaptation(&mut resin);
    //     let (value, n_ops) = resin.circuits[0].lock().unwrap().get_value();
    //     operations.push(n_ops);

    //     match clock.elapsed() {
    //         Ok(elapsed) => {
    //             times.push(elapsed.as_secs_f64());
    //             if elapsed.as_secs() > 5 { break; }
    //         }
    //         Err(e) => {
    //             println!("Error {e:?}");
    //             break;
    //         }
    //     }
    // }

    // let mut plot = Plot::new();
    // plot.add_trace(Scatter::new(times, operations));
    // plot.set_layout(Layout::new()
    //     .title(Title::new("Sales Data"))
    //     .x_axis(PAxis::new().title(Title::new("Time / s")))
    //     .y_axis(PAxis::new().title(Title::new("#Operations")))
    // );
    // plot.write_html("output/operations_curve.html");

    Ok(())

    // loop {
    //     println!("Value of RC = {:?}", resin.circuits[0].lock().unwrap().get_value());
    //     retreive_messages();
    // }

    // Ok(())
}
