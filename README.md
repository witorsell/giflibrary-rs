# giflibrary-rs

a lightweight image and gif upload server that automatically converts static images into discord compatible animated webps. rewritten in rust for blazing fast performance.

## features

- discord friendly: converts all static images (jpegs pngs and webps) to animated 2 frame webps automatically so they can be favorited in the discord client natively
- autocomplete ready: built in suggestion and tag endpoints ready for discord bot integration
- secure: uploads and suggestions are locked behind a master passphrase. anti ddos measures and upload limits keep the server safe
- cloudflare r2 powered: completely stateless architecture backing up straight to r2

## how it works

it takes your uploads processes them on the fly using ffmpeg and pushes them straight to a cloudflare r2 bucket. static images get duplicated into a 2 frame webp loop because discord refuses to let you favorite static images directly from url embeds. videos get converted too.


## setup

you need rust and ffmpeg installed

```bash
git clone https://github.com/vangeldrako/giflibrary-rs
cd giflibrary-rs
cargo build --release
```

make a `.env` file and put your master passphrase and r2 keys in it like this

```
PORT=3000
CONTACT_EMAIL=your_email@example.com
MASTER_KEY=your_secret_password_here
R2_ENDPOINT=your_r2_endpoint
R2_ACCESS_KEY_ID=your_access_key
R2_SECRET_ACCESS_KEY=your_secret_key
R2_BUCKET=your_bucket_name
R2_PUBLIC_URL=https://your-public-r2-url.com
```

then start it up

```bash
cargo run --release
```
or use pm2 if you want it to run in the background

```bash
pm2 start ./target/release/giflibrary-rs --name giflibrary
```

## license

released under the "it's not like i made this for you" public license (inlimtfypl). see `license` for details. but don't get the wrong idea i didn't write it because i wanted to help you i just had some free time okay
