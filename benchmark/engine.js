const { Database } = require('../index');
const bunyan = require('bunyan');
const { hrtime } = require('process');
const crypto = require('crypto');

const getRandomBytes = size => crypto.randomBytes(size);

let counter = 0;
let db;

const currentMicros = () =>  {
    let hrTime = process.hrtime()
    return hrTime[0] * 1000000 + hrTime[1] / 1000;
}

const logger = bunyan.createLogger({
    name: 'benchmark',
    streams: [
        {
            level: 'info',
            stream: process.stdout            // log INFO and above to stdout
        },
        {
            level: 'info',
            path: __dirname + '/benchmark.log'  // log ERROR and above to a file
        }
    ]
});

let availableData = [];
let requestQueue = {};
let total = 0;

const requestData = key => {
    let resolve, reject;
    const resp = new Promise((iResolve, iReject) => {
        resolve = iResolve;
        reject = iReject;
    });

    counter += 1;
    process.send({
        type: 'get',
        id: counter,
        key: key.toString('hex'),
    });

    requestQueue[counter] = { resolve, reject };
    return resp;
};

const requestSetData = data => {
    let resolve, reject;
    const resp = new Promise((iResolve, iReject) => {
        resolve = iResolve;
        reject = iReject;
    });

    counter += 1;
    process.send({
        type: 'set',
        id: counter,
        data: data.map(kv => ({ key: kv.key.toString('hex'), value: kv.value.toString('hex')})),
    });

    requestQueue[counter] = { resolve, reject };
    return resp;
};

const getRandomInt = (max) =>
    Math.floor(Math.random() * max);


const fetchNum = 10000;

setInterval(async () => {
    availableData = [];
    for (let i = 0; i < fetchNum; i++) {
    availableData.push({ key: getRandomBytes(100) })
    }
    const now = currentMicros();
    for (let i = 0; i < fetchNum; i++) {
        await requestData(availableData[getRandomInt(availableData.length)].key);
    }
    logger.warn({ diff: currentMicros() - now, total }, `fetch ${fetchNum}`);
}, 3000);

const writeNum = 10000;

// setInterval(async () => {
//     const data = [];
//     for (let i = 0; i < writeNum; i++) {
//         data.push({
//             key: getRandomBytes(100),
//             value: getRandomBytes(1000),
//         });
//     }
//     const now = currentMicros();
//     await requestSetData(data);
//     logger.warn({ diff: currentMicros() - now, total }, `set ${writeNum}`);
//     availableData.push(...data);
//     total += writeNum;
// }, 5000);

process.on('message', msg => {
    if (msg.type === 'written') {
        total += 1;
        availableData.push({
            key: Buffer.from(msg.pair.key, 'hex'),
            value: Buffer.from(msg.pair.value, 'hex'),
        });
        return;
    }
    if (msg.type === 'get_resp') {
        const req = requestQueue[msg.id];
        if (req) {
            if (msg.value) {
                req.resolve(Buffer.from(msg.value, 'hex'));
            } else {
                req.reject(msg.err);
            }
            delete requestData[msg.id];
        }
    }
    if (msg.type === 'set_resp') {
        const req = requestQueue[msg.id];
        if (req) {
            if (!msg.err) {
                req.resolve();
            } else {
                req.reject(msg.err);
            }
            delete requestData[msg.id];
        }
    }
});

// (async () => {
//     while (true) {
//         if (availableData.length > 100) {
//             const now = currentMicros();
//             for (let i = 0; i < 100; i++) {
//                 await requestData(availableData[getRandomInt(availableData.length)]);
//             }
//             logger.info({ diff: currentMicros() - now }, 'fetch 100');
//         }
//     }
// })()