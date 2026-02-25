use std::{time::Instant};

use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener};
use tonic::transport::Channel;
use tonic::Request;
use axum::{Json, Router, extract::{State}, routing::{get, post}};
use orderbook_proto::{CancelOrderRequest, CancelOrderResponse, ModifyOrderRequest, ModifyOrderResponse, NewOrderRequest, NewOrderResponse, OrderBookClient, orders::OrderType, BookRequest};
use prometheus::{HistogramOpts, HistogramVec, IntCounter, Registry, TextEncoder};
use lazy_static::lazy_static;

lazy_static!(
    static ref NEW_ORDER_TOTAL_DURATION : HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "new_order_total_duration_ms", 
            "total time from http request to response for new order")
        .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]), 
        &["order-type", "status"]
    ).unwrap();

    static ref CANCEL_ORDER_TOTAL_DURATION : HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "cancel_order_total_duration_ms", 
            "total time from http request to response for cancel order")
        .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]), 
        &["status"]
    ).unwrap();

    static ref MODIFY_ORDER_TOTAL_DURATION : HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "modify_order_total_duration_ms", 
            "total time from http request to response for modify order")
        .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]), 
        &["status"]
    ).unwrap();

    static ref DEPTH_TOTAL_DURATION : HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "depth_total_duration_ms", 
            "total time from http request to response for depth")
        .buckets(vec![1.0, 5.0, 10.0, 25.0, 50.0, 100.0]), 
        &["asset_name", "status"]
    ).unwrap();

    static ref REQUEST_COUNTER : IntCounter = IntCounter::new("request_counter", "total no of requests").unwrap();
);

#[derive(Debug, Clone)]
pub struct SharedState{
    pub client : OrderBookClient<Channel>,
    registry : Registry
}

impl SharedState {
    pub async fn new() -> Result<Self, anyhow::Error>{
        let client = OrderBookClient::connect("http://[::1]:50051").await?;
        let registry = Registry::new();
        Ok(Self { client, registry })
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {

    let shared_state = SharedState::new().await?;
    let app = Router::new()
    .route("/new", post(new_order))
    .route("/modify", post(modify_order))
    .route("/cancel", post(cancel_order))
    .route("/depth", get(depth))
    .route("/metric", get(metric))
    .with_state(shared_state);
    
    let listener = TcpListener::bind("127.0.0.1:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

async fn new_order(
    State(mut shared_state) : State<SharedState>, // axum clones the client instance for us over here.
    Json(request) : Json<NewOrder>, ) -> Json<NewOrderRes>{
        let start_time = Instant::now();
        let req = request;
        let order_request = Request::new(NewOrderRequest{
            user_id : None,
            price : req.price,
            quantity : req.quantity,
            is_buy_side : req.is_buy_side,
            security_name : req.security_name,
            order_type : OrderType::Market as i32
        });
        
        let response = shared_state.client.new_order(order_request).await.unwrap().into_inner();
        let res_to_send = NewOrderRes::from(response);
        let total_duration = start_time.elapsed().as_millis() as f64;
        NEW_ORDER_TOTAL_DURATION.with_label_values(&["New Order", "success"]).observe(total_duration);
        REQUEST_COUNTER.inc();
        Json(res_to_send)
}

async fn modify_order(
    State(mut shared_state) : State<SharedState>, 
    Json(request) : Json<ModifyOrder>) -> Json<ModifyOrderRes>{
        let start_time = Instant::now();
       
        let req = request;
        let new_price = if req.new_price.unwrap() == 0 {None} else { req.new_price};
        let new_quantity = if req.new_quantity.unwrap() == 0 {None} else { req.new_quantity};

        let modify_request = Request::new(ModifyOrderRequest {
            order_id : req.order_id,
            new_price,
            new_quantity,
            side : req.is_buy_side
        });
    let response = shared_state.client.modify_order(modify_request).await.unwrap().into_inner();
    let converted_res = ModifyOrderRes::from(response);
    let total_time = start_time.elapsed().as_millis() as f64;
    MODIFY_ORDER_TOTAL_DURATION.with_label_values(&[converted_res.status.to_string()]).observe(total_time);
    REQUEST_COUNTER.inc();
    Json(converted_res) 

}
async fn cancel_order(
    State(mut shared_state) : State<SharedState>,
    Json(request) : Json<CancelOrder>) -> Json<CancelOrderRes> {
        let start_time = Instant::now();
        let req = request;
        let cancel_request = Request::new(CancelOrderRequest{
            order_id : req.order_id
        });
    let response = shared_state.client.cancel_order(cancel_request).await.unwrap().into_inner();
    let converted_res = CancelOrderRes::from(response);
    let total_time = start_time.elapsed().as_millis() as f64;
    CANCEL_ORDER_TOTAL_DURATION.with_label_values(&[converted_res.status.to_string()]).observe(total_time);
    REQUEST_COUNTER.inc();
    Json(converted_res)
}

async fn depth(
    State(mut shared_state) : State<SharedState>,
    Json(request) : Json<DepthReq>
) -> Json<DepthRes>{
    let start_time = Instant::now();
    let req = request;
    let security_name = req.security_name.clone();
    let level_count = if req.level_count.unwrap() == 0{
        None
    } else {
        req.level_count
    };
    let depth_request = Request::new(BookRequest{
        security_name : req.security_name,
        level_count
    });
    let response = shared_state.client.book_depth(depth_request).await.unwrap().into_inner();
    let book_depth = response.book_depth;
    let total_time = start_time.elapsed().as_millis() as f64;
    REQUEST_COUNTER.inc();
    match book_depth{
        Some(book) => {
            DEPTH_TOTAL_DURATION.with_label_values(&[security_name, 200.to_string()]).observe(total_time);
            println!("-------ASK-------");
            for e in &book.ask_depth{
                println!("[price = {}, quantity = {}]", e.price,e.quantity)
            }
            println!("------BID------");
            for e in &book.bid_depth{
                println!("[price = {}, quantity = {}]", e.price,e.quantity)
            }
            Json(DepthRes{
                status : 200,
                output : "book recieved".to_string()
            })
        }
        None => {
            DEPTH_TOTAL_DURATION.with_label_values(&[security_name, 400.to_string()]).observe(total_time);
            println!("book is empty");
            Json(DepthRes { status: 400, output: "orderbook is empty".to_string() })
        }
    }
}

async fn metric(State(shared_state): State<SharedState>) -> String{
    let metric_families = shared_state.registry.gather();
    let encoder = TextEncoder::new();
    encoder.encode_to_string(&metric_families).unwrap()
}

#[derive(Debug, Deserialize)]
pub struct NewOrder{
    pub price : Option<u32>,
    pub quantity : u32,
    pub is_buy_side : bool,
    pub security_name : String,
}

#[derive(Debug, Serialize)]
pub struct NewOrderRes{
    pub order_id : String,
    pub status : u32,
    pub order_index : Option<u32>,
    pub cause : Option<String>
}

impl From<orderbook_proto::NewOrderResponse> for NewOrderRes {
    fn from(value: NewOrderResponse) -> Self {
        Self { order_id: value.order_id, status: value.status, order_index: value.order_index, cause: value.cause }
    }
}

#[derive(Debug, Deserialize)]
pub struct ModifyOrder{
    pub order_id : String,
    pub new_price : Option<u32>,
    pub new_quantity : Option<u32>,
    pub is_buy_side : bool,
}

#[derive(Debug, Serialize)]
pub struct ModifyOrderRes{
    pub order_id : String,
    pub status : u32,
    pub output : Option<String>
}

impl From<ModifyOrderResponse> for ModifyOrderRes {
    fn from(value: ModifyOrderResponse) -> Self {
        Self { order_id: value.order_id, status: value.status, output: value.output }
    }
}

#[derive(Debug, Deserialize)]
pub struct CancelOrder{
    pub order_id : String
}

#[derive(Debug, Serialize)]
pub struct CancelOrderRes{
    pub order_id : String,
    pub status : u32,
    pub output : Option<String>
}

impl From<CancelOrderResponse> for CancelOrderRes {
    fn from(value: CancelOrderResponse) -> Self {
        Self { order_id : value.order_id, status : value.status, output : value.cause}
    }
}

#[derive(Debug, Deserialize)]
pub struct DepthReq{
    security_name : String,
    level_count : Option<u32>
}

#[derive(Debug, Serialize)]
pub struct DepthRes{
    status : u32,
    output : String
}
