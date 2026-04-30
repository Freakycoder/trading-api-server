use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clob_engine::{Tracing, order_book::types::CancelOutcome};
use clob_engine::order_book::types::{EngineNewOrder, EngineCancelOrder, EngineModifyOrder, OrderType};
use lazy_static::lazy_static;
use clob_engine::MatchingEngine;
use prometheus::{HistogramOpts, HistogramVec, IntCounter, Registry, TextEncoder};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap, sync::{Arc, atomic::{AtomicU64, Ordering}}, thread, time::Instant
};
use tracing::{Span, field::Empty};
use tokio::net::TcpListener;
use crossbeam::channel::{Sender as BufferSender, bounded};
use tokio::sync::oneshot::{channel, Sender as OneshotSender};

lazy_static! {
    static ref NEW_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "new_order_total_duration_ms",
            "total time from http request to response for new order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["order_type", "status"]
    )
    .unwrap();
    static ref CANCEL_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "cancel_order_total_duration_ms",
            "total time from http request to response for cancel order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["status"]
    )
    .unwrap();
    static ref MODIFY_ORDER_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "modify_order_total_duration_ms",
            "total time from http request to response for modify order"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["status"]
    )
    .unwrap();
    static ref DEPTH_TOTAL_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "depth_total_duration_ms",
            "total time from http request to response for depth"
        )
        .buckets(vec![0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 10.0, 25.0, 50.0, 100.0]),
        &["asset_name", "status"]
    )
    .unwrap();
    static ref REQUEST_COUNTER: IntCounter =
        IntCounter::new("request_counter", "total no of requests").unwrap();
}

static ORDER_ID_COUNTER : AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct SharedState {
    pub security_registery : Arc<HashMap<String, u32>>,
    pub registry: Registry,
    pub buffer_sender : BufferSender<OrderEvent>
}

impl SharedState {
    pub async fn new(sender : BufferSender<OrderEvent>) -> Result<Self, anyhow::Error> {

        let registry = Registry::new();
        let mut security_registery = HashMap::new();
        security_registery.insert("btc".to_string(), 1);
        security_registery.insert("eth".to_string(), 2);
        let security_registery = Arc::new(security_registery);
        Ok(Self {
            security_registery,
            registry,
            buffer_sender : sender
        })
    }
}

pub struct OrderError(anyhow::Error);

impl IntoResponse for OrderError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response() // axum has implemented into_response for different tuple combinations like (statuscode, json/html/string/headermap)
    }
}

#[derive(Debug)]
pub enum OrderEvent {
    NewOrder(EngineNewOrder,Span, OneshotSender<Result<NewOrderRes, String>>),
    ModifyOrder(EngineModifyOrder,Span, OneshotSender<Result<ModifyOrderRes,String>>),
    CancelOrder(EngineCancelOrder,Span, OneshotSender<Result<CancelOrderRes,String>>),
    DepthOrder(EngineDepthRequest, Span, OneshotSender<Result<DepthRes, String>>)
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {

    let (sender , consumer) = bounded::<OrderEvent>(1000);
    let shared_state =  SharedState::new(sender).await?;
    let _ = shared_state
        .registry
        .register(Box::new(NEW_ORDER_TOTAL_DURATION.clone()));
    let _ = shared_state
        .registry
        .register(Box::new(CANCEL_ORDER_TOTAL_DURATION.clone()));
    let _ = shared_state
        .registry
        .register(Box::new(MODIFY_ORDER_TOTAL_DURATION.clone()));
    let _ = shared_state
        .registry
        .register(Box::new(REQUEST_COUNTER.clone()));

    let app = Router::new()
        .route("/new", post(new_order))
        .route("/modify", post(modify_order))
        .route("/cancel", post(cancel_order))
        .route("/depth", post(depth))
        .route("/metrics", get(metric))
        .with_state(shared_state);

    thread::spawn(move || {
        let mut engine = MatchingEngine::new();
        loop {
            match consumer.recv(){
                Ok(message) => {
                    match message{
                        OrderEvent::NewOrder(new_order,span ,producer ) => {
                            let order_id = new_order.engine_order_id;
                            match engine.match_order(new_order, &span){
                                Ok(potential_index) => {
                                    match potential_index{
                                        Some(index) => {
                                            let _ = producer.send(Ok(NewOrderRes { order_id: order_id.to_string(), status: 200, order_index: Some(index as u32), cause: None }));
                                        }
                                        None => {
                                            let _ = producer.send(Ok(NewOrderRes { order_id: order_id.to_string(), status: 200, order_index: None, cause: Some("order got consumed by the book".to_string()) }));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = producer.send(Err(e.to_string()));
                                }
                            }
                        }
                        OrderEvent::ModifyOrder(modify_order,span ,producer ) => {
                            match engine.modify(modify_order.order_id, modify_order.security_id, modify_order.new_price, modify_order.new_quantity, modify_order.is_buy_side, &span){
                                Ok(outcome) => {
                                    let _ = producer.send(Ok(ModifyOrderRes {
                                        order_id : modify_order.order_id.to_string(),
                                        status : 200,
                                        output : Some(outcome.to_string())
                                    }));
                                }
                                Err(e) => {
                                    let _ = producer.send(Err(e.to_string()));
                                }
                            }
                        }
                        OrderEvent::CancelOrder(cancel_order,span ,producer ) => {
                            match engine.cancel(cancel_order.order_id, cancel_order.security_id, &span, cancel_order.is_buy_side){
                                Ok(outcome) => {
                                    match outcome {
                                        CancelOutcome::Success => {
                                            let _ = producer.send(Ok(CancelOrderRes {
                                                order_id : cancel_order.order_id.to_string(),
                                                status : 200,
                                                output : Some(format!("order cancelled succesfully"))
                                            }));
                                        }
                                        CancelOutcome::Failed => {
                                            let _ = producer.send(Ok(CancelOrderRes {
                                                order_id : cancel_order.order_id.to_string(),
                                                status : 200,
                                                output : Some(format!("order consumed or modified"))
                                            }));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = producer.send(Err(e.to_string()));
                                }
                            } 
                        }
                        OrderEvent::DepthOrder(depth_order,span ,producer ) => {
                            match engine.depth(depth_order.security_id, depth_order.level_count, &span){
                                Ok(outcome) => {
                                    let _ = producer.send(Ok(DepthRes {
                                        status : 200,
                                        ask_depth : outcome.ask_depth.into_iter().map(|level| PriceLevel {
                                            price : level.price_level,
                                            quantity : level.quantity
                                        }).collect(),
                                        bid_depth : outcome.bid_depth.into_iter().map(|level| PriceLevel {
                                            price : level.price_level,
                                            quantity : level.quantity
                                        }).collect()
                                    }));
                                }
                                Err(e) => {
                                    let _ = producer.send(Err(e.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    std::hint::spin_loop();
                }
            }
        }
    });
    let listener = TcpListener::bind("0.0.0.0:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

#[axum::debug_handler]
async fn new_order(
    State(shared_state): State<SharedState>, // axum clones the client instance for us over here.
    Json(request): Json<NewOrder>,
) -> Result<Json<NewOrderRes>, OrderError> {
    let start_time = Instant::now();
    let (oneshot_sender, reciever) = channel::<Result<NewOrderRes, String>>(); // ONESHOT channel being created seperately in every fn
    let req = request;
    if req.price.is_none() && req.order_type == "limit" {
        return Err(OrderError(anyhow!("Price cannot be 0")));
    }
    if req.quantity == 0 {
        return Err(OrderError(anyhow!("Quantity cannot be 0")));
    }

    let security_id = shared_state
        .security_registery
        .get(&req.security_name)
        .ok_or_else(|| OrderError(anyhow!("unknown security: {}", req.security_name)))?;

    let order_id = ORDER_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

    let match_span = Tracing::match_order_span(
        order_id,
        Empty,
        Empty,
        if req.is_buy_side { "buy" } else { "sell" },
        req.is_buy_side,
        Empty,
        Empty,
        Empty,
    );

    let order_request = EngineNewOrder {
    engine_order_id: order_id,
    price: req.price,
    initial_quantity: req.quantity,
    current_quantity: req.quantity,
    is_buy_side: req.is_buy_side,
    security_id: *security_id,
    order_type: if req.order_type == "market" {
        OrderType::Market(req.price)
    } else {
        OrderType::Limit
    },
};

    REQUEST_COUNTER.inc();
    let order_type = if req.is_buy_side { "buy" } else { "sell" };

    let crossbeam_sender = shared_state.buffer_sender;
    crossbeam_sender.send(OrderEvent::NewOrder(order_request, match_span, oneshot_sender)).map_err(|e| OrderError(anyhow!("{}",e)))?;

    match reciever.await {
        Ok(result) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            match result{
                Ok(message) => {
                    NEW_ORDER_TOTAL_DURATION
                .with_label_values(&[order_type, "200"])
                .observe(total_duration);
                    return Ok(Json(message))
                }
                Err(e) => {
                    NEW_ORDER_TOTAL_DURATION
                .with_label_values(&[order_type, "500"])
                .observe(total_duration);
                    return Err(OrderError(anyhow!("{}",e)))
                }
            }
        }
        Err(e) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            NEW_ORDER_TOTAL_DURATION
                .with_label_values(&[order_type, "400"])
                .observe(total_duration);
            Err(OrderError(anyhow!(format!(
                "error in recieveing the message from buffer due to : {}",
                e
            ))))
        }
    }
}

#[axum::debug_handler]
async fn modify_order(
    State(shared_state): State<SharedState>,
    Json(request): Json<ModifyOrder>,
) -> Result<Json<ModifyOrderRes>, OrderError> {
    let start_time = Instant::now();

    let req = request;
    let new_price = req.new_price.filter(|&p| p > 0);
    let new_quantity = req.new_quantity.filter(|&p| p > 0);

    if new_price.is_none() && new_quantity.is_none() {
        return Err(OrderError(anyhow!("both price and quantity cannot be empty")));
    }

    let order_id = req.order_id;
    let security_id = shared_state
        .security_registery
        .get(&req.security_name)
        .ok_or_else(|| OrderError(anyhow!("unknown security: {}", req.security_name)))?;

    let modify_span = Tracing::modify_span(
        order_id,
        false,
        Empty,
        Empty,
        Empty,
        if req.is_buy_side { "buy" } else { "sell" },
        req.is_buy_side,
        0,
        0,
    );

    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<ModifyOrderRes, String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::ModifyOrder(EngineModifyOrder {
                    order_id,
                    new_price,
                    new_quantity,
                    is_buy_side: req.is_buy_side,
                    security_id : *security_id
                }, modify_span, oneshot_sender)).map_err(|e| OrderError(anyhow!("{}",e)))?;

    match receiver.await {
        Ok(result) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            match result{
                Ok(message) => {
                    MODIFY_ORDER_TOTAL_DURATION
                .with_label_values(&[message.status.to_string().as_str()])
                .observe(total_duration);
                    return Ok(Json(message))
                }
                Err(e) => {
                    MODIFY_ORDER_TOTAL_DURATION
                .with_label_values(&["500"])
                .observe(total_duration);
                    return Err(OrderError(anyhow!("{}",e)));
                }
            }
        }
        Err(e) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            MODIFY_ORDER_TOTAL_DURATION
                .with_label_values(&["400"])
                .observe(total_duration);
            Err(OrderError(anyhow!("receiver error: {}", e)))
        }
    }
}
#[axum::debug_handler]
async fn cancel_order(
    State(shared_state): State<SharedState>,
    Json(request): Json<CancelOrder>,
) -> Result<Json<CancelOrderRes>, OrderError> {
    let start_time = Instant::now();
    let req = request;
    let order_id = req.order_id;
    let security_id = shared_state
        .security_registery
        .get(&req.security_name)
        .ok_or_else(|| OrderError(anyhow!("unknown security: {}", req.security_name)))?;

    let cancel_span = Tracing::cancel_span(order_id, false, "");


    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<CancelOrderRes,String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::CancelOrder(EngineCancelOrder { order_id, is_buy_side : req.is_buy_side ,security_id : *security_id }, 
        cancel_span, 
        oneshot_sender)).map_err(|e| OrderError(anyhow!("{}",e)))?;

    match receiver.await {
        Ok(result) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            match result{
                Ok(message) => {
                    CANCEL_ORDER_TOTAL_DURATION
                .with_label_values(&[message.status.to_string().as_str()])
                .observe(total_duration);
                    return Ok(Json(message))
                }
                Err(e) => {
                    CANCEL_ORDER_TOTAL_DURATION
                .with_label_values(&["500"])
                .observe(total_duration);
                    return Err(OrderError(anyhow!("{}",e)));
                }
            }
        }
        Err(e) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            CANCEL_ORDER_TOTAL_DURATION
                .with_label_values(&["400"])
                .observe(total_duration);
            Err(OrderError(anyhow!("receiver error: {}", e)))
        }
    }
}

#[axum::debug_handler]
async fn depth(
    State(shared_state): State<SharedState>,
    Json(request): Json<DepthReq>,
) -> Result<Json<DepthRes>, OrderError> {
    let start_time = Instant::now();
    let req = request;

    let security_id = shared_state
        .security_registery
        .get(&req.security_name)
        .ok_or_else(|| OrderError(anyhow!("unknown security: {}", req.security_name)))?;

    let level_count = req.level_count.filter(|&l| l > 0);

    let depth_span = Tracing::depth_span(Empty, Empty, Empty);

    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<DepthRes, String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::DepthOrder(EngineDepthRequest {
                    security_id: *security_id,
                    level_count}, 
                    depth_span, 
                    oneshot_sender)).map_err(|e| OrderError(anyhow!("{}",e)))?;

    match receiver.await {
        Ok(result) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            
            match result{
                Ok(message) => {
                    DEPTH_TOTAL_DURATION
                .with_label_values(&[req.security_name.as_str(), message.status.to_string().as_str()])
                .observe(total_duration);
                    return Ok(Json(message))
                }
                Err(e) => {
                    DEPTH_TOTAL_DURATION
                .with_label_values(&[req.security_name.as_str(), "500"])
                .observe(total_duration);
                    return Err(OrderError(anyhow!("{}",e)));
                }
            }
        }
        Err(e) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            DEPTH_TOTAL_DURATION
                .with_label_values(&[req.security_name.as_str(), "400"])
                .observe(total_duration);
            Err(OrderError(anyhow!("receiver error: {}", e)))
        }
    }
}

async fn metric(State(shared_state): State<SharedState>) -> String {
    let _ = shared_state
        .registry
        .register(Box::new(NEW_ORDER_TOTAL_DURATION.clone()));

    let metric_families = shared_state.registry.gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metric_families).unwrap()
}

#[derive(Debug, Deserialize)]
pub struct NewOrder {
    pub price: Option<u32>,
    pub quantity: u32,
    pub is_buy_side: bool,
    pub security_name: String,
    pub order_type: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct NewOrderRes {
    pub order_id: String,
    pub status: u32,
    pub order_index: Option<u32>,
    pub cause: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ModifyOrder {
    pub order_id: u64,
    pub security_name : String,
    pub new_price: Option<u32>,
    pub new_quantity: Option<u32>,
    pub is_buy_side: bool,
}

#[derive(Debug, Serialize)]
pub struct ModifyOrderRes {
    pub order_id: String,
    pub status: u32,
    pub output: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CancelOrder {
    pub order_id: u64,
    pub security_name : String,
    pub is_buy_side : bool
}

#[derive(Debug, Serialize)]
pub struct CancelOrderRes {
    pub order_id: String,
    pub status: u32,
    pub output: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DepthReq {
    security_name: String,
    level_count: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DepthRes {
    pub status: u16,
    pub ask_depth: Vec<PriceLevel>,
    pub bid_depth: Vec<PriceLevel>,
}

#[derive(Debug, Serialize)]
pub struct PriceLevel {
    pub price: u32,
    pub quantity: u32,
}

#[derive(Debug)]
pub struct EngineDepthRequest {
    pub security_id: u32,
    pub level_count: Option<u32>,
}