import { createIPX, ipxFSStorage, serveIPX } from "ipx";
import sharp from "sharp";

sharp.cache(false);
const storage = ipxFSStorage({ dir: "/images", maxAge: 0 });
serveIPX(createIPX({ storage, maxAge: 0 }), { hostname: "0.0.0.0", port: 8080 });
