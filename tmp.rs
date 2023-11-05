
fn randomized_study() {
    println!("Building randomized RC.");
    let experiment_time = 20;
    let number_leafs = 2500;
    let number_models = 5000;
    let number_inferences = 2000;
    let mut rc = randomized_rc(number_leafs, number_models);

    println!("Activate randomized IPC.");
    let distribution = SkewNormal::new(0.1, 3.0, -1.0).unwrap();
    let mut true_frequencies = vec![];
    for index in 0..number_leafs {
        let channel = format!("leaf_{}", rc.foliage.lock().unwrap()[index].name);
        activate_channel(rc.foliage.clone(), index, &channel, &false);

        let mut rng = rand::thread_rng();
        let mut frequency = distribution.sample(&mut rng);
        if frequency < 0.001 {
            frequency = 0.001;
        }
        true_frequencies.push(frequency as f64);
        let new_publisher = RandomizedIpcChannel::new(
            &rc.foliage.lock().unwrap()[index]
                .ipc_channel
                .as_ref()
                .unwrap()
                .topic,
            true_frequencies[true_frequencies.len() - 1],
            rng.gen_range(0.1..1.0),
        );
        new_publisher.unwrap().start();
    }
    let mut sorted_frequencies = true_frequencies.clone();
    sorted_frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("F = {:?}", sorted_frequencies);
    let true_frequencies_array = Array1::from(true_frequencies);

    // let mut operations = vec![];
    // let mut operation_ratios = vec![];
    // let mut max_operations = 0.0;
    // let mut mse = vec![];
    // let mut inference_timestamps = vec![];
    // let mut adaptation_timestamps = vec![];

    let mut inference_times = vec![];
    let mut values = vec![];
    let mut adaptation_times = vec![];

    inference_times.reserve(number_inferences * 2);
    values.reserve(number_inferences * 2);

    println!("Loop original for {} steps.", number_inferences);
    // let _ = message_loop(rc.foliage.clone());

    while values.len() < number_inferences {
        retreive_messages();

        let before = Instant::now();
        let value = rc.value();
        let elapsed = before.elapsed().as_secs_f64();

        // if n_ops == 0 {
        //     continue;
        // }

        // let value = rc.value();

        // println!("Inference took {elapsed}s");
        inference_times.push(elapsed);
        values.push(value);

        // inference_timestamps.push(runtime_clock.elapsed().as_secs_f64());

        if values.len() % 100 == 0 {
            println!("Done {}", values.len());
        }

        // if operations.is_empty() {
        //     max_operations = n_ops as f64;
        // }
        // operations.push(n_ops);
        // operation_ratios.push(n_ops as f64 / max_operations);

        // let frequencies: Vec<f64> = rc
        //     .foliage
        //     .lock()
        //     .unwrap()
        //     .iter()
        //     .map(|leaf| leaf.get_frequency())
        //     .collect();

        // mse.push(
        //     Array1::from(frequencies)
        //         .mean_squared_error(&true_frequencies_array)
        //         .unwrap(),
        // );
    }

    // adaptation_timestamps.push(runtime_clock.elapsed().as_secs_f64());
    let before: Instant = Instant::now();
    frequency_adaptation(&mut rc);
    adaptation_times.push(before.elapsed().as_secs_f64());
    println!(
        "#Adaptations in {}s",
        adaptation_times[adaptation_times.len() - 1]
    );

    // println!("Loop adapted for {}s.", experiment_time);
    // let runtime_clock = Instant::now();
    // loop {
    //     retreive_messages();

    //     let before = Instant::now();
    //     let (value, n_ops) = rc.counted_value();
    //     let elapsed = before.elapsed().as_secs_f64();

    //     // if n_ops == 0 {
    //     //     continue;
    //     // }
    //     // println!("Inference took {elapsed}s");
    //     inference_times.push(elapsed);
    //     inference_timestamps.push(runtime_clock.elapsed().as_secs_f64() + experiment_time as f64);

    //     values.push(value);
    //     if operations.is_empty() {
    //         max_operations = n_ops as f64;
    //     }
    //     operations.push(n_ops);
    //     operation_ratios.push(n_ops as f64 / max_operations);

    //     if runtime_clock.elapsed().as_secs() >= experiment_time {
    //         break;
    //     }
    // }

    let mut deploy = rc.deploy();
    deploy.reverse();
    println!("Loop deployed for {} steps.", number_inferences);
    while values.len() < 2 * number_inferences {
        retreive_messages();

        let before = Instant::now();
        let foliage_guard = rc.foliage.lock().unwrap();
        deploy.par_iter_mut().for_each(|memory| {
            memory.value(&foliage_guard);
        });
        // let n_ops = deploy.par_iter_mut().fold(|| 0, |acc, memory| acc + memory.val(&foliage_guard).1).sum::<usize>();
        let value = deploy[0].value(&foliage_guard);
        drop(foliage_guard);
        let elapsed = before.elapsed().as_secs_f64();

        values.push(value);
        if values.len() % 100 == 0 {
            println!("Done {}", values.len());
        }

        inference_times.push(elapsed);

        // if n_ops == 0 {
        //     continue;
        // }

        // if operations.is_empty() {
        //     max_operations = n_ops as f64;
        // }
        // operations.push(n_ops);
        // operation_ratios.push(n_ops as f64 / max_operations);

        // inference_timestamps.push(runtime_clock.elapsed().as_secs_f64() + experiment_time as f64);
    }

    // let mut plot = Plot::new();
    // plot.add_trace(
    //     Scatter::new(inference_timestamps.to_vec(), operation_ratios).name("Operations Ratio"),
    // );
    // plot.add_trace(
    //     Scatter::new(inference_timestamps.clone(), values.clone())
    //         .name("Value"),
    // );
    // plot.set_layout(
    //     Layout::new()
    //         .title(Title::new("Reactive Inference"))
    //         .x_axis(
    //             PAxis::new()
    //                 .title(Title::new("Time / s"))
    //                 .range(vec![0, 2 * experiment_time]),
    //         )
    //         .y_axis(
    //             PAxis::new()
    //                 .title(Title::new("Operations Ratio"))
    //                 .range(vec![0, 1]),
    //         )
    // );
    // plot.write_html("output/operations.html");

    // plot.write_html("output/values.html");
    // let mut plot = Plot::new();
    // plot.add_trace(Scatter::new(inference_timestamps.clone(), mse.clone()));
    // plot.set_layout(
    //     Layout::new()
    //         .title(Title::new("Reactive Inference"))
    //         .x_axis(
    //             PAxis::new()
    //                 .title(Title::new("Time / s"))
    //                 .range(vec![0, 2 * experiment_time]),
    //         )
    //         .y_axis(PAxis::new().title(Title::new("MSE of Estimated FoC"))),
    // );
    // plot.write_html("output/mse.html");

    // plot = Plot::new();
    // plot.add_trace(
    //     Scatter::new(inference_timestamps.to_vec(), inference_times.clone()).name("Inference Time"),
    // );
    // plot.add_trace(
    //     Scatter::new(
    //         inference_timestamps[window_size / 2..].to_vec(),
    //         averaged_inference_times.clone(),
    //     )
    //     .name("Avg. Inference Time"),
    // );
    // plot.set_layout(
    //     Layout::new()
    //         .title(Title::new("Reactive Inference"))
    //         .x_axis(
    //             PAxis::new()
    //                 .title(Title::new("Time / s"))
    //                 .range(vec![0, 2 * experiment_time]),
    //         )
    //         .y_axis(PAxis::new().title(Title::new("Inference Time / s")))
    // );
    // plot.write_html("output/time.html");

    // plot = Plot::new();
    // plot.add_trace(Bar::new(vec![0, 1], vec![sum_inference_before, sum_inference_after]));
    // plot.write_html("output/oaverall_time.html");

    use std::fs::File;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;

    println!("Export results.");

    let path = Path::new("output/inference_times.csv");
    if !path.exists() {
        let mut file = File::create(path).expect("Unable to create file");
        file.write_all("Time,Runtime,Leafs\n".as_bytes());
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let mut csv_text = "".to_string();
    for i in 0..inference_times.len() {
        csv_text.push_str(&format!(
            "{},{},{}\n",
            i,
            inference_times[i],
            number_leafs / 2 * number_models
        ));
    }
    file.write_all(csv_text.as_bytes())
        .expect("Unable to write data");
}
