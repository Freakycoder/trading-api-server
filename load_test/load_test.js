import http from "k6/http";
import {Client} from 'k6/experimental/redis'
import { check, sleep } from "k6";
import { Trend, Counter, Rate } from "k6/metrics";

export const options = {
    scenarios : {
        low_producers : {
            executor : "constant-arrival-rate",
            rate : 10,
            exec : "producerVU",
            timeUnit : "1s",
            duration : "30s",
            preAllocatedVUs : 20,
            maxVUs : 50,
            startTime : "0s",
            tags : {stage : "low_10rps"}
        },
        low_consumer : {
            executor : "constant-arrival-rate",
            rate : 10,
            exec : "consumerVU",
            timeUnit : "1s",
            duration : "15s",
            preAllocatedVUs : 20,
            maxVUs : 50,
            startTime : "15s",
            tags : {stage : "low_10rps"}
        },
        medium_producer : {
            executor : "constant-arrival-rate",
            rate : 100,
            exec : "producerVU",
            timeUnit : "1s",
            duration : "30s",
            preAllocatedVUs : 120,
            maxVUs : 150,
            startTime : "40s",
            tags : {stage : "meduim_100rps"}
        },
        medium_consumer : {
            executor : "constant-arrival-rate",
            rate : 100,
            exec : "consumerVU",
            timeUnit : "1s",
            duration : "15s",
            preAllocatedVUs : 120,
            maxVUs : 150,
            startTime : "55s",
            tags : {stage : "meduim_100rps"}
        },
        high_producer : {
            executor : "constant-arrival-rate",
            rate : 500,
            exec : "producerVU",
            timeUnit : "1s",
            duration : "30s",
            preAllocatedVUs : 500,
            maxVUs : 700,
            startTime : "50s",
            tags : {stage : "high_500rps"}
        },
        high_consumer : {
            executor : "constant-arrival-rate",
            rate : 500,
            exec : "consumerVU",
            timeUnit : "1s",
            duration : "15s",
            preAllocatedVUs : 500,
            maxVUs : 700,
            startTime : "65s",
            tags : {stage : "high_500rps"}
        },
        spike_producer : {
            executor : "constant-arrival-rate",
            rate : 1000,
            exec : "producerVU",
            timeUnit : "1s",
            duration : "10s",
            preAllocatedVUs : 1000,
            maxVUs : 1500,
            startTime : "60s",
            tags : {stage : "spike_1000rps"}
        },
        spike_consumer : {
            executor : "constant-arrival-rate",
            rate : 1000,
            exec : "consumerVU",
            timeUnit : "1s",
            duration : "5s",
            preAllocatedVUs : 1000,
            maxVUs : 1500,
            startTime : "65s",
            tags : {stage : "spike_1000rps"}
        }
    }
};

const redis = new Client('redis://localhost:6379');

export async function producerVU(){
    const res = http.post("http://localhost:8000/new", {
        security_name : getSecurity(),
        price : getPrice(),
        quantity : getQuantity(),
        is_buy_side : getSide()
    })
    const order_id = res.json("order_id");
    await redis.lpush("order_ids", order_id);
}

export async function consumerVU(){
    const list_length = await redis.llen("order_ids");
    if (list_length == 0) {
        sleep(0.1);
        return
    }
    const random_idx = Math.floor(Math.random() * list_length);
    const order_id = await redis.lindex("order_ids", random_idx);
    const action = Math.random();

    if (action > 0 && action <= 0.2){
        //modify
        let res = http.post("http://localhost:8000/modify", {
            order_id,
            new_price : setPrice(),
            new_quantity : setQuantity(),
            is_buy_side : getSide()
        });
        let parsed_res = res.json();
    }
    else if (action > 0.2 && action <= 0.5 ) {
        //cancel
        let res = http.post("http://localhost:8000/cancel", {
            order_id
        });
        let parsed_res = res.json();
        if (parsed_res.status) {
            await redis.lrem("order_ids", 1 , order_id);
        }
    }
    else {
        // do nothing
        return
    }
}

function setPrice(){
    return Math.floor() < 0.5 ? 0 : getPrice()
}

function setQuantity(){
    return Math.floor() < 0.5 ? 0 : getQuantity()
}

function getQuantity(){
    return Math.floor(Math.random() * 1000)
}

function getPrice(){
    return Math.floor(Math.random() * 100)
}

function getSide(){
    return Math.random() < 0.5 ? true : false
}

function getSecurity(){
    return Math.random() < 0.5 ? "btc" : "eth"
}