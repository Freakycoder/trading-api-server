use axum::{http::StatusCode, response::{IntoResponse, Response}};
use crossbeam::channel::Sender as BufferSender;
use prometheus::Registry;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot::Sender as OneshotSender;
use std::{collections::HashMap, sync::Arc, time::Instant};
use clob_engine::{EngineCancelOrder, EngineNewOrder, EngineModifyOrder};


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

pub struct OrderError(pub anyhow::Error);

impl IntoResponse for OrderError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0.to_string()).into_response() // axum has implemented into_response for different tuple combinations like (statuscode, json/html/string/headermap)
    }
}

#[derive(Debug)]
pub enum OrderEvent {
    NewOrder(EngineNewOrder,OneshotSender<Result<NewOrderRes, String>>, Instant),
    ModifyOrder(EngineModifyOrder, OneshotSender<Result<ModifyOrderRes,String>>, Instant),
    CancelOrder(EngineCancelOrder, OneshotSender<Result<CancelOrderRes,String>>, Instant),
    DepthOrder(EngineDepthRequest, OneshotSender<Result<DepthRes, String>>, Instant)
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
    pub orders_touched : u32,
    pub levels_consumed : u32,
    pub timer : f64
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
    pub security_name: String,
    pub level_count: Option<u32>,
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