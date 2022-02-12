const { Database } = require("./index");

(async () => {
    const db = new Database('./tmp', {readonly: false });
    console.log(db);
    const res = await db.get(Buffer.alloc(0));
    db.close();
})()