const { Database } = require('../index');
const bunyan = require('bunyan');
const { hrtime } = require('process');


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

process.on('message', msg => {
    if (msg.type === 'init') {
        db = new Database('./.tmp', { readonly: true, count: msg.count });
        process.send({type: 'ready'});
        return;
    }
    counter += 1;
    if (counter === 100000) {
        const now = currentMicros();
        db.catchup()
            .catch(logger.error)
            .finally(() => {
                counter = 0;
                const diff = currentMicros() - now;
                logger.warn({ diff }, 'catchup');
            });
    }
    const key = Buffer.from(msg.pair.key, 'hex');
    db.exists(key)
        .then(exist => {
            logger.info({ exist, key: msg.pair.key }, 'exist');
        })
        .catch(logger.error);

    // const reader = db.iterate();
    // const res = new Promise(resolve => {
    //     const data = [];
    //     reader
    //         .on('data', d => {
    //             data.push(d);
    //         })
    //         .on('end', () => {
    //             resolve(data);
    //         });
    // });
    // res.then(v => console.log('total', v.length));
});