use std::env;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use clearscreen::clear;
use serde_json::{json, Value};
use chrono::prelude::*;

fn get_cpu_times() -> (Vec<u64>, Vec<Vec<u64>>) {
    let file = File::open("/proc/stat").unwrap();
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let cpu_times: Vec<u64> = lines.next().unwrap().unwrap()
        .split_whitespace()
        .skip(1)
        .map(|x| x.parse().unwrap())
        .collect();

    let mut core_times = Vec::new();
    for line in lines {
        let line = line.unwrap();
        if line.starts_with("cpu") {
            let times: Vec<u64> = line
                .split_whitespace()
                .skip(1)
                .map(|x| x.parse().unwrap())
                .collect();
            core_times.push(times);
        } else {
            break;
        }
    }

    (cpu_times, core_times)
}

fn calculate_time_diff(prev: &[u64], current: &[u64]) -> Vec<i64> {
    current.iter().zip(prev.iter())
        .map(|(curr, prev)| *curr as i64 - *prev as i64)
        .collect()
}

fn store_values(cpu_times: &[u64], core_times: &[Vec<u64>], stored_values: &mut Vec<(u64, Vec<u64>, Vec<Vec<u64>>)>) {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    stored_values.push((timestamp, cpu_times.to_vec(), core_times.to_vec()));
}


fn read_json_file(timestamp: Option<&str>) {
    let file = File::open("cpu_averages.json").unwrap();
    let reader = BufReader::new(file);

    let mut json_data = String::new();
    for line in reader.lines() {
        json_data.push_str(&line.unwrap());
    }

    let parsed_data: Value = serde_json::from_str(&json_data).unwrap();

    if let Some(timestamp) = timestamp {
        if let Some(data) = parsed_data.as_array().and_then(|arr| arr.iter().find(|obj| obj[timestamp].is_object())) {
            print_json_data(timestamp, &data[timestamp]);
        } else {
            println!("No data found for timestamp {}.", timestamp);
        }
    } else {
        if let Some(data_array) = parsed_data.as_array() {
            for data in data_array {
                for (timestamp, values) in data.as_object().unwrap() {
                    print_json_data(timestamp, values);
                }
            }
        }
    }
}

fn format_timestamp(timestamp: u64) -> String {
    let datetime: DateTime<Local> = DateTime::from(UNIX_EPOCH + Duration::from_secs(timestamp));
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}



fn print_values(stored_values: &Vec<(u64, Vec<u64>, Vec<Vec<u64>>)>, print_avg: bool, num_times: usize) {
    let num_values = stored_values.len();

    if num_values < 2 {
        println!("Not enough stored values to print differences.");
        return;
    }

    let start_index = if print_avg { num_values.saturating_sub(num_times) } else { num_values - 2 };
    let actual_times = if print_avg { num_values - start_index } else { 1 };
    
    let mut cpu_avgs = vec![0i64; 10];
    let mut core_avgs: Vec<Vec<i64>> = vec![vec![0; 10]; stored_values[0].2.len()];

    for window in stored_values.windows(2).skip(start_index) {
        let (_, prev_cpu, prev_cores) = &window[0];
        let (_, curr_cpu, curr_cores) = &window[1];

        let cpu_diff = calculate_time_diff(prev_cpu, curr_cpu);
        for (i, diff) in cpu_diff.iter().enumerate() {
            cpu_avgs[i] += *diff;
        }

        for (j, (prev_core, curr_core)) in prev_cores.iter().zip(curr_cores.iter()).enumerate() {
            let core_diff = calculate_time_diff(prev_core, curr_core);
            for (i, diff) in core_diff.iter().enumerate() {
                core_avgs[j][i] += *diff;
            }
        }
    }

    // Calculate averages
    if print_avg {
        for avg in &mut cpu_avgs {
            *avg /= actual_times as i64;
        }
        for core_avg in &mut core_avgs {
            for avg in core_avg {
                *avg /= actual_times as i64;
            }
        }
    }

    println!("CPU time differences (jiffies){}:", if print_avg { " (average)" } else { "" });
    println!("{:>5} {:>10} {:>10} {:>10} {:>10} {:>10} {:>5} {:>10} {:>10} {:>10} {:>10}", 
             "CPU", "user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal", "guest", "guest_nice");
    
    print!("{:<5}", "cpu");
    for avg in cpu_avgs.iter() {
        print!("{:>10}", avg);
    }
    println!();

    for (i, core_avg) in core_avgs.iter().enumerate() {
        print!("{:<5}", format!("cpu{}", i));
        for avg in core_avg.iter() {
            print!("{:>10}", avg);
        }
        println!();
    }
    println!();

    // Calculate and print percentage usage only if not in average mode
    if !print_avg {
        println!("CPU Usage Percentages:");
        println!("{:>5} {:>10}", "CPU", "Usage %");
        
        let total_time: i64 = cpu_avgs.iter().sum();
        let idle_time = cpu_avgs[3] + cpu_avgs[4];
        let usage_percent = (1.0 - idle_time as f32 / total_time as f32) * 100.0;
        println!("{:<5} {:>10.2}%", "avg", usage_percent);

        for (i, core_avg) in core_avgs.iter().enumerate() {
            let total_time: i64 = core_avg.iter().sum();
            let idle_time = core_avg[3] + core_avg[4];
            let usage_percent = (1.0 - idle_time as f32 / total_time as f32) * 100.0;
            println!("{:<5} {:>10.2}%", format!("cpu{}", i), usage_percent);
        }
        println!();
    }

    // Store to JSON file (unchanged)
    let json_data = json!({
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string(): {
            "cpu": cpu_avgs,
            "cores": core_avgs
        }
    });

    let file_path = "cpu_averages.json";
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(file_path)
        .unwrap();

    if file.metadata().unwrap().len() == 0 {
        write!(file, "[").unwrap();
    } else {
        file.seek(SeekFrom::End(-1)).unwrap();
        write!(file, ",").unwrap();
    }

    writeln!(file, "{}", json_data.to_string()).unwrap();
    write!(file, "]").unwrap();
}

fn print_json_data(timestamp: &str, data: &Value) {
    let timestamp_readable = format_timestamp(timestamp.parse().unwrap());
    println!("\n{}:", timestamp_readable);
    println!("CPU time differences (jiffies):");
    println!("     user     nice   system    idle   iowait     irq  softirq   steal   guest guest_nice");
    
    if let Some(cpu) = data["cpu"].as_array() {
        print!("cpu");
        for value in cpu {
            print!(" {:8}", value.as_i64().unwrap());
        }
        println!();
    }

    if let Some(cores) = data["cores"].as_array() {
        for (i, core) in cores.iter().enumerate() {
            print!("cpu{:<2}", i);
            if let Some(core_values) = core.as_array() {
                for value in core_values {
                    print!(" {:8}", value.as_i64().unwrap());
                }
            }
            println!();
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let print_avg = args.contains(&"--avg".to_string());
    let num_times = args.iter()
        .position(|arg| arg == "--times")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    if args.contains(&"--read".to_string()) {
        let timestamp = args.iter().position(|arg| arg == "--read").and_then(|i| args.get(i + 1));
        read_json_file(timestamp.map(|x| x.as_str()));
        return;
    }

    let mut stored_values = Vec::new();

    loop {
        let start = Instant::now();

        let (cpu_times, core_times) = get_cpu_times();

        store_values(&cpu_times, &core_times, &mut stored_values);

        if !print_avg {
            clear().expect("Failed to clear screen");
        }

        print_values(&stored_values, print_avg, num_times);

        // Keep only the last 'num_times' measurements if print_avg is true
        if print_avg && stored_values.len() > num_times {
            stored_values.remove(0);
        } else if !print_avg && stored_values.len() > 2 {
            stored_values.remove(0);
        }

        let elapsed = start.elapsed();
        if elapsed < Duration::from_secs(1) {
            thread::sleep(Duration::from_secs(1) - elapsed);
        }
    }
}