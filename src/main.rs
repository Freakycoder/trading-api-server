use anyhow::anyhow;
use axum::{
    Json, Router,
    extract::{State,},
    routing::{get, post},
};
use clob_engine::{EngineNewOrder, order_book::types::CancelOutcome};
use clob_engine::order_book::types::{ EngineCancelOrder, EngineModifyOrder, OrderType};
use clob_engine::MatchingEngine;
use prometheus::{TextEncoder};
use std::{
    sync::{atomic::{AtomicU64, Ordering}}, thread, time::Instant
};
use tokio::net::TcpListener;
use crossbeam::channel::{bounded};
use tokio::sync::oneshot::{channel};
mod histogram;
use histogram::{CANCEL_ORDER_TOTAL_DURATION, NEW_ORDER_TOTAL_DURATION, MODIFY_ORDER_TOTAL_DURATION, 
    REQUEST_COUNTER, DEPTH_TOTAL_DURATION, ORDERS_TOUCHED, LEVELS_CONSUMED, INCOMING_ORDER_QUANTITY,
    ORDER_MATCH_DURATION, MODIFY_DURATION, CANCEL_DURATION, DEPTH_DURATION, QUEUE_WAIT_TIME
};
use crate::types::{CancelOrder, CancelOrderRes, DepthReq, DepthRes, ModifyOrder, ModifyOrderRes, NewOrder, NewOrderRes, OrderError, OrderEvent, PriceLevel, SharedState};
mod types;

static ORDER_ID_COUNTER : AtomicU64 = AtomicU64::new(1);

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
    let _ = shared_state.registry.register(Box::new(INCOMING_ORDER_QUANTITY.clone()));
    let _ = shared_state.registry.register(Box::new(ORDERS_TOUCHED.clone()));
    let _ = shared_state.registry.register(Box::new(LEVELS_CONSUMED.clone()));
    let _ = shared_state.registry.register(Box::new(ORDER_MATCH_DURATION.clone()));
    let _ = shared_state.registry.register(Box::new(MODIFY_DURATION.clone()));
    let _ = shared_state.registry.register(Box::new(CANCEL_DURATION.clone()));
    let _ = shared_state.registry.register(Box::new(QUEUE_WAIT_TIME.clone()));

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
                        OrderEvent::NewOrder(new_order ,producer , queue_timer) => {
                            QUEUE_WAIT_TIME.observe(queue_timer.elapsed().as_micros() as f64);
                            let order_id = new_order.engine_order_id;
                            match engine.match_order(new_order){
                                Ok(potential_index) => {
                                    match potential_index.order_index{
                                        Some(index) => {
                                            let _ = producer.send(Ok(NewOrderRes { order_id: order_id.to_string(), status: 200, order_index: Some(index as u32), cause: None, orders_touched : potential_index.orders_touched, levels_consumed : potential_index.levels_consumed, timer : potential_index.timer}));
                                        }
                                        None => {
                                            let _ = producer.send(Ok(NewOrderRes { order_id: order_id.to_string(), status: 200, order_index: None, cause: Some("order got consumed by the book".to_string()), orders_touched : potential_index.orders_touched, levels_consumed : potential_index.levels_consumed, timer : potential_index.timer }));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = producer.send(Err(e.to_string()));
                                }
                            }
                        }
                        OrderEvent::ModifyOrder(modify_order ,producer , queue_timer) => {
                            QUEUE_WAIT_TIME.observe(queue_timer.elapsed().as_micros() as f64);
                            let timer = Instant::now();
                            match engine.modify(modify_order.order_id, modify_order.security_id, modify_order.new_price, modify_order.new_quantity, modify_order.is_buy_side){
                                Ok(outcome) => {
                                    let elapsed_time = timer.elapsed().as_micros() as f64;
                                    MODIFY_DURATION.observe(elapsed_time);
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
                        OrderEvent::CancelOrder(cancel_order ,producer, queue_timer ) => {
                            QUEUE_WAIT_TIME.observe(queue_timer.elapsed().as_micros() as f64);
                            match engine.cancel(cancel_order.order_id, cancel_order.security_id, cancel_order.is_buy_side){
                                Ok(outcome) => {
                                    match outcome {
                                        CancelOutcome::Success(timer) => {
                                            CANCEL_DURATION.observe(timer);
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
                        OrderEvent::DepthOrder(depth_order ,producer , queue_timer) => {
                            QUEUE_WAIT_TIME.observe(queue_timer.elapsed().as_micros() as f64);
                            match engine.depth(depth_order.security_id, depth_order.level_count,){
                                Ok(outcome) => {
                                    DEPTH_DURATION.observe(outcome.timer);
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
                    // do nothing
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

    INCOMING_ORDER_QUANTITY.with_label_values(&[&req.security_name]).observe(req.quantity as f64);
    REQUEST_COUNTER.inc();
    let order_type = if req.is_buy_side { "buy" } else { "sell" };

    let crossbeam_sender = shared_state.buffer_sender;
    crossbeam_sender.send(OrderEvent::NewOrder(order_request, oneshot_sender, Instant::now())).map_err(|e| OrderError(anyhow!("{}",e)))?;

    match reciever.await {
        Ok(result) => {
            let total_duration = start_time.elapsed().as_millis() as f64;
            match result{
                Ok(message) => {
                    NEW_ORDER_TOTAL_DURATION
                        .with_label_values(&[order_type, "200"])
                        .observe(total_duration);
                    ORDERS_TOUCHED.with_label_values(&[&req.security_name]).observe(message.orders_touched as f64);
                    LEVELS_CONSUMED.with_label_values(&[&req.security_name]).observe(message.levels_consumed as f64);
                    ORDER_MATCH_DURATION.observe(message.timer);
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

    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<ModifyOrderRes, String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::ModifyOrder(EngineModifyOrder {
                    order_id,
                    new_price,
                    new_quantity,
                    is_buy_side: req.is_buy_side,
                    security_id : *security_id
                }, oneshot_sender, Instant::now())).map_err(|e| OrderError(anyhow!("{}",e)))?;

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

    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<CancelOrderRes,String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::CancelOrder(EngineCancelOrder { order_id, is_buy_side : req.is_buy_side ,security_id : *security_id}, 
        oneshot_sender, Instant::now())).map_err(|e| OrderError(anyhow!("{}",e)))?;

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

    REQUEST_COUNTER.inc();

    let (oneshot_sender, receiver) = channel::<Result<DepthRes, String>>();
    let buffer_sender = shared_state.buffer_sender;
    buffer_sender.send(OrderEvent::DepthOrder(types::EngineDepthRequest {
                    security_id: *security_id,
                    level_count}, 
                    oneshot_sender, Instant::now())).map_err(|e| OrderError(anyhow!("{}",e)))?;

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