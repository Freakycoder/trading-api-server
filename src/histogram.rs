use lazy_static::lazy_static;
use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounter};

lazy_static! {
    pub static ref NEW_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "new_order_total_duration_ms",
            "total time from http request to response for new order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["order_type", "status"]
    )
    .unwrap();
    pub static ref CANCEL_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "cancel_order_total_duration_ms",
            "total time from http request to response for cancel order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["status"]
    )
    .unwrap();
    pub static ref MODIFY_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "modify_order_total_duration_ms",
            "total time from http request to response for modify order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["status"]
    )
    .unwrap();
    pub static ref DEPTH_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "depth_total_duration_ms",
            "total time from http request to response for depth"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["asset_name", "status"]
    )
    .unwrap();
    pub static ref REQUEST_COUNTER: IntCounter =
        IntCounter::new("request_counter", "total no of requests").unwrap();

    pub static ref INCOMING_ORDER_QUANTITY : HistogramVec = HistogramVec::new(
        HistogramOpts::new("order_qty", "the quatity of the order arrived")
        .buckets(vec![1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2000.0]),
        &["asset_name"]
    ).unwrap();

    pub static ref ORDERS_TOUCHED: HistogramVec = HistogramVec::new(
        HistogramOpts::new("orders_touched", "orders touched per match")
        .buckets(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0, 50.0]),
        &["asset_name"]
    ).unwrap();

    pub static ref LEVELS_CONSUMED: HistogramVec = HistogramVec::new(
        HistogramOpts::new("levels_consumed", "levels consumed per match")
        .buckets(vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 7.0, 10.0, 15.0]),
        &["asset_name"]
    ).unwrap();

    pub static ref ORDER_MATCH_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "order_match_duration",
            "pure matching engine processing time for new orders in microseconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0])
    ).unwrap();

    pub static ref CANCEL_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "cancel_duration",
            "pure matching engine processing time for cancel orders in microseconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0])
    ).unwrap();

    pub static ref MODIFY_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "modify_duration",
            "pure matching engine processing time for modify orders in microseconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0])
    ).unwrap();
    pub static ref DEPTH_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "depth_duration",
            "pure matching engine processing time for depth queries in microseconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0])
    ).unwrap();

    pub static ref QUEUE_WAIT_TIME: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "queue_wait_time",
            "time order spent waiting in channel before engine picked it up in microseconds"
        )
        .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 500.0, 1000.0])
    ).unwrap();
}