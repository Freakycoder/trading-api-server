use serde::{Deserialize, Serialize};
use tokio::net::{TcpListener};
use tonic::transport::Channel;
use tonic::Request;
use axum::{Json, Router, extract::{State}, routing::{get, post}};
use orderbook_proto::{CancelOrderRequest, CancelOrderResponse, ModifyOrderRequest, ModifyOrderResponse, NewOrderRequest, NewOrderResponse, OrderBookClient, orders::OrderType, BookRequest};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {

    let client = OrderBookClient::connect("http://[::1]:50051").await?;

    let app = Router::new()
    .route("/new", post(new_order))
    .route("/modify", post(modify_order))
    .route("/cancel", post(cancel_order))
    .route("/depth", get(depth))
    .with_state(client);
    
    let listener = TcpListener::bind("127.0.0.1:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

async fn new_order(
    State(mut client) : State<OrderBookClient<Channel>>, // axum clones the client instance for us over here.
    Json(request) : Json<NewOrder>, ) -> Json<NewOrderRes>{
        let req = request;
        let order_request = Request::new(NewOrderRequest{
            user_id : None,
            price : req.price,
            quantity : req.quantity,
            is_buy_side : req.is_buy_side,
            security_name : req.security_name,
            order_type : OrderType::Market as i32
        });
        
        let response = client.new_order(order_request).await.unwrap().into_inner();
        let res_to_send = NewOrderRes::from(response);
        Json(res_to_send)
}

async fn modify_order(
    State(mut client) : State<OrderBookClient<Channel>>, 
    Json(request) : Json<ModifyOrder>) -> Json<ModifyOrderRes>{
        let req = request;
        let modify_request = Request::new(ModifyOrderRequest {
            order_id : req.order_id,
            new_price : req.new_price,
            new_quantity : req.new_quantity,
            side : req.is_buy_side
        });
    let response = client.modify_order(modify_request).await.unwrap().into_inner();
    let converted_res = ModifyOrderRes::from(response);
    Json(converted_res) 

}
async fn cancel_order(
    State(mut client) : State<OrderBookClient<Channel>>,
    Json(request) : Json<CancelOrder>) -> Json<CancelOrderRes> {
        let req = request;
        let cancel_request = Request::new(CancelOrderRequest{
            order_id : req.order_id
        });
    let response = client.cancel_order(cancel_request).await.unwrap().into_inner();
    let converted_res = CancelOrderRes::from(response);
    Json(converted_res)
}

async fn depth(
    State(mut client) : State<OrderBookClient<Channel>>,
    Json(request) : Json<DepthReq>
) -> Json<DepthRes>{
    let req = request;
    let level_count = if req.level_count.unwrap() == 0{
        None
    } else {
        req.level_count
    };
    let depth_request = Request::new(BookRequest{
        security_name : req.security_name,
        level_count
    });
    let response = client.book_depth(depth_request).await.unwrap().into_inner();
    let book_depth = response.book_depth;
    match book_depth{
        Some(book) => {
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
            println!("book is empty");
            Json(DepthRes { status: 400, output: "orderbook is empty".to_string() })
        }
    }
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
