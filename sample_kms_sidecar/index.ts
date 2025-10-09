import express from "express";
import { fromBase64 } from "@iota/iota-sdk/utils";
import { Secp256k1PublicKey } from "@iota/iota-sdk/keypairs/secp256k1";
import { getPublicKey, signAndVerify } from "./awsUtils.js";

async function main() {
    const app = express();
    app.use(express.json());
    const port = 3000;
    app.get("/", (req, res) => {
        res.send("KMS Signer Demo!");
    });

    app.get("/get-pubkey-address", async (req, res) => {
        try {
            const keyId = process.env.AWS_KMS_KEY_ID || "";
            const publicKey = await getPublicKey(keyId);
            const publicKeyToUse = publicKey instanceof Secp256k1PublicKey
                ? publicKey
                : undefined;
            const iotaPubkeyAddress = publicKeyToUse?.toIotaAddress();
            res.json({ iotaPubkeyAddress: iotaPubkeyAddress });
        } catch (error) {
            console.error(error);
            res.status(500).send("Internal server error");
        }
    });

    app.post("/sign-transaction", async (req, res) => {
        try {
            const { txBytes } = req.body;

            if (!txBytes) {
                return res
                    .status(400)
                    .send("Missing transaction bytes or keyId");
            }

            const txBytesArray = fromBase64(txBytes);
            const signature = await signAndVerify(txBytesArray);

            res.json({ signature });
        } catch (error) {
            console.error(error);
            res.status(500).send("Internal server error");
        }
    });

    app.listen(port, () => {
        console.log(`Example app listening at http://localhost:${port}`);
    });
}

main();
