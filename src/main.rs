// Download daily historical Euribor rate data from 
// https://www.bundesbank.de/en/statistics/money-and-capital-markets/interest-rates-and-yields/money-market-rates-651538
// and store the files in the directory of Cargo.toml as:
//    "BBIG1.D.D0.EUR.MMKT.EURIBOR.W01.BID._Z.csv"
//    "BBIG1.D.D0.EUR.MMKT.EURIBOR.M01.BID._Z.csv"
//    "BBIG1.D.D0.EUR.MMKT.EURIBOR.M03.BID._Z.csv"
//    "BBIG1.D.D0.EUR.MMKT.EURIBOR.M06.BID._Z.csv"
//    "BBIG1.D.D0.EUR.MMKT.EURIBOR.M12.BID._Z.csv"
//
// Then run this program with:
//    cargo run 'days'
// where 'days' is the number of days for the forward average rate.
//
// The program will create a file "euribor_cost_chart.html".

use chrono::{NaiveDate, Duration};
use csv::ReaderBuilder;
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;

const NUM_RATES: usize = 5;

#[derive(Debug, Clone)]
struct EuriborRate {
    date: NaiveDate,
    rate: f64,
}

#[derive(Debug)]
struct AllEuriborRates {
    w01: Vec<EuriborRate>,
    m01: Vec<EuriborRate>,
    m03: Vec<EuriborRate>,
    m06: Vec<EuriborRate>,
    m12: Vec<EuriborRate>,
}

// Read a CSV file and return a vector of EuriborRate structs
fn read_csv(path: &str) -> Result<Vec<EuriborRate>, Box<dyn Error>> {
    let file = File::open(path)?;
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(file);

    let mut rates = Vec::new();
    let mut records = reader.records();
    let mut last_valid_rate: Option<f64> = None;

    // Skip the first 9 lines (metadata)
    for _ in 0..9 {
        records.next();
    }

    for result in records {
        let record = result?;
        if record.len() < 2 {
            continue;
        }
        
        let date = NaiveDate::parse_from_str(&record[0], "%Y-%m-%d")?;
        let rate_str = record[1].trim();
        
        let rate = if rate_str == "." || rate_str.is_empty() || rate_str.to_lowercase().contains("no value") {
            last_valid_rate
        } else {
            match rate_str.parse::<f64>() {
                Ok(r) => {
                    last_valid_rate = Some(r);
                    Some(r)
                }
                Err(_) => last_valid_rate
            }
        };
        
        if let Some(r) = rate {
            rates.push(EuriborRate { date, rate: r });
        }
    }

    Ok(rates)
}

// Calculate average rates and determine the averaged time mark
fn calculate_average_rates(all_rates: &AllEuriborRates, averaged_time_days: i64) -> (Vec<[f64; NUM_RATES]>, NaiveDate) {
    let rates_vec = [
        &all_rates.w01, &all_rates.m01, &all_rates.m03,
        &all_rates.m06, &all_rates.m12
    ];
    let periods = [7, 30, 90, 180, 360];

    let start_date = rates_vec.iter()
        .filter_map(|r| r.first())
        .min_by_key(|r| r.date)
        .map(|r| r.date)
        .unwrap();
    let end_date = rates_vec.iter()
        .filter_map(|r| r.last())
        .max_by_key(|r| r.date)
        .map(|r| r.date)
        .unwrap();

    let rate_maps: Vec<HashMap<NaiveDate, f64>> = rates_vec.iter()
        .map(|rates| rates.iter().map(|r| (r.date, r.rate)).collect())
        .collect();

    let mut averages = Vec::new();
    let mut current_date = start_date;
    let averaged_time_mark = end_date - Duration::days(averaged_time_days);

    while current_date <= end_date {
        let mut avg_rates = [0.0; NUM_RATES];

        for i in 0..NUM_RATES {
            let period = periods[i];
            let mut sum = 0.0;
            let mut total_days = 0;
            let mut check_date = current_date;
            let days_left = (end_date - current_date).num_days() + 1;
            let check_period = std::cmp::min(averaged_time_days, days_left as i64);

            while check_date <= current_date + Duration::days(check_period - 1) {
                if let Some(&rate) = rate_maps[i].get(&check_date) {
                    let days_in_period = std::cmp::min(period, (end_date - check_date).num_days() as i64 + 1);
                    sum += rate * days_in_period as f64;
                    total_days += days_in_period;
                }
                check_date += Duration::days(period);
            }

            avg_rates[i] = if total_days > 0 { sum / total_days as f64 } else { 0.0 };
        }

        averages.push(avg_rates);
        current_date += Duration::days(1);
    }

    (averages, averaged_time_mark)
}

// Create the chart data for Plotly
fn create_chart_data(all_rates: &AllEuriborRates, averages: &[[f64; NUM_RATES]], averaged_time_mark: NaiveDate, averaged_time_days: i64) -> Result<serde_json::Value, Box<dyn Error>> {
    let labels = ["1w", "1m", "3m", "6m", "12m"];
    let colors = ["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd"];
    let mut traces = Vec::new();

    let rates_vec = [
        &all_rates.w01, &all_rates.m01, &all_rates.m03,
        &all_rates.m06, &all_rates.m12
    ];

    let start_date = rates_vec.iter()
        .filter_map(|r| r.first())
        .min_by_key(|r| r.date)
        .map(|r| r.date)
        .unwrap();

    for i in 0..NUM_RATES {
        // Average rates trace
        let avg_trace = json!({
            "x": (0..averages.len()).map(|j| (start_date + Duration::days(j as i64)).format("%Y-%m-%d").to_string()).collect::<Vec<String>>(),
            "y": averages.iter().map(|a| a[i]).collect::<Vec<f64>>(),
            "type": "scattergl",
            "mode": "lines",
            "name": format!("{} ({}d rlz avg)", labels[i], averaged_time_days),
            "line": {
                "color": colors[i],
                "width": 2
            }
        });
        traces.push(avg_trace);

        // Daily rates trace
        let daily_trace = json!({
            "x": rates_vec[i].iter().map(|r| r.date.format("%Y-%m-%d").to_string()).collect::<Vec<String>>(),
            "y": rates_vec[i].iter().map(|r| r.rate).collect::<Vec<f64>>(),
            "type": "scattergl",
            "mode": "lines",
            "name": format!("{} (daily value)", labels[i]),
            "line": {
                "color": colors[i],
                "width": 1,
                "dash": "dot"
            }
        });
        traces.push(daily_trace);
    }

    // Add vertical line for the averaged time mark
    let max_rate = rates_vec.iter()
        .flat_map(|rates| rates.iter().map(|r| r.rate))
        .fold(f64::NEG_INFINITY, f64::max);

    let vertical_line = json!({
        "x": [averaged_time_mark.format("%Y-%m-%d").to_string(), averaged_time_mark.format("%Y-%m-%d").to_string()],
        "y": [0, max_rate],
        "type": "scatter",
        "mode": "lines",
        "name": "Full forward data end point",
        "line": {
            "color": "gray",
            "width": 1,
            "dash": "dash"
        },
        "showlegend": true
    });
    traces.push(vertical_line);

    Ok(json!(traces))
}

// Generate the HTML content for the chart
fn generate_html(chart_data: &serde_json::Value, averaged_time_days: i64) -> String {
    format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>Euribor Rates Chart</title>
    <script src="https://cdn.plot.ly/plotly-latest.min.js"></script>
    <style>
        #chart {{ width: 100%; height: 800px; }}
    </style>
</head>
<body>
    <div id="chart"></div>
    <script>
        var data = {0};
        var layout = {{
            title: 'Euribor rates\' {1}-day forward realized cost (average interest rate)',
            showlegend: true,
            xaxis: {{ 
                title: 'Date', 
                type: 'date',
                rangeslider: {{visible: true}}
            }},
            yaxis: {{ 
                title: 'Interest rate (%)',
                dtick: 0.5
            }},
            dragmode: 'zoom'
        }};
        var config = {{
            scrollZoom: true,
            modeBarButtonsToAdd: ['drawline', 'drawopenpath', 'drawclosedpath', 'drawcircle', 'drawrect', 'eraseshape']
        }};
        
        Plotly.newPlot('chart', data, layout, config);
    </script>
</body>
</html>
    "#, chart_data, averaged_time_days)
}

fn main() -> Result<(), Box<dyn Error>> {
    // Get the averaged time period from command line arguments or use default
    let args: Vec<String> = env::args().collect();
    let averaged_time_days = if args.len() > 1 {
        args[1].parse().unwrap_or(360)
    } else {
        360
    };
    
    let file_names = [
        "BBIG1.D.D0.EUR.MMKT.EURIBOR.W01.BID._Z.csv",
        "BBIG1.D.D0.EUR.MMKT.EURIBOR.M01.BID._Z.csv",
        "BBIG1.D.D0.EUR.MMKT.EURIBOR.M03.BID._Z.csv",
        "BBIG1.D.D0.EUR.MMKT.EURIBOR.M06.BID._Z.csv",
        "BBIG1.D.D0.EUR.MMKT.EURIBOR.M12.BID._Z.csv",
    ];

    let mut all_rates = AllEuriborRates {
        w01: Vec::new(),
        m01: Vec::new(),
        m03: Vec::new(),
        m06: Vec::new(),
        m12: Vec::new(),
    };

    println!("Reading CSV files...\n");
    for (i, file_name) in file_names.iter().enumerate() {
        println!("Reading {}...", file_name);
        let rates = read_csv(file_name)
            .map_err(|e| format!("Failed to read CSV {}: {}", file_name, e))?;
        
        println!("Total records: {}", rates.len());
        println!("First record:");
        for rate in rates.iter().take(1) {
            println!(" Date: {}, Rate: {}", rate.date, rate.rate);
        }
        println!("Last record:");
        for rate in rates.iter().rev().take(1) {
            println!(" Date: {}, Rate: {}", rate.date, rate.rate);
        }
        println!();

        if rates.is_empty() {
            return Err(format!("No valid rates found in the CSV file: {}", file_name).into());
        }

        match i {
            0 => all_rates.w01 = rates,
            1 => all_rates.m01 = rates,
            2 => all_rates.m03 = rates,
            3 => all_rates.m06 = rates,
            4 => all_rates.m12 = rates,
            _ => unreachable!(),
        }
    }

    println!("Calculating average rates for the forward period of {} days...", averaged_time_days);
    let (averages, averaged_time_mark) = calculate_average_rates(&all_rates, averaged_time_days);
    
    println!("Creating chart data...");
    let chart_data = create_chart_data(&all_rates, &averages, averaged_time_mark, averaged_time_days)?;
    
    println!("Generating HTML content...");
    let html_content = generate_html(&chart_data, averaged_time_days);
    
    println!("Writing HTML file...");
    let mut file = File::create("euribor_cost_chart.html")?;
    write!(file, "{}", html_content)?;

    println!("Chart created successfully: euribor_cost_chart.html");
    Ok(())
}
