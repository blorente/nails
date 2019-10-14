use std::default::Default;
use std::fmt::Debug;
use std::io;
use std::path::PathBuf;

use futures::{future, Future, Stream, Sink};
use futures::sync::mpsc;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use tokio_core::reactor::Handle;

use codec::{Codec, InputChunk, OutputChunk};
use execution::{Args, ChildInput, ChildOutput, child_channel, send_to_io};

/// TODO Rename to Stdin/Server output chunk.
#[derive(Debug)]
enum Event {
    Server(OutputChunk),
    Stdin,
}

struct ClientState<C: ServerSink>(C);

pub fn execute<T>(
    handle: Handle,
    transport: Framed<T, Codec>,
) -> IOFuture<()>
where
    T: AsyncRead + AsyncWrite + Debug + 'static,
{
    let (server_write, server_read) = transport.split();

    // Select on the two input sources to create a merged Stream of events.
    // TODO: Handle stdin with this `select`.
    let events_read = futures::stream::empty()
        .then(|res| match res {
            Ok(v) => Ok(Event::Stdin),
            Err(e) => Err(err(&format!("Failed to emit child output: {:?}", e))),
        })
        .select(server_read.map(|e| Event::Server(e)));

    // Send all the init chunks

    Box::new(
        events_read
            .fold(ClientState(server_write), move |state, ev| step(&handle, state, ev))
            .then(|_| Ok(())),
    )
}

fn step<C: ServerSink>(
    handle: &Handle,
    state: ClientState,
    ev: Event,
) -> IOFuture<ClientState<C>> {
    match ev {
        // TODO This blocks because it uses std::io, we should switch it to tokio::io
        Event::Server(OutputChunk::Stderr(bytes)) => 
            future::result(io::stderr().write_all(bytes))
                    .map(|_| state),
        Event::Server(OutputChunk::Stdout(bytes)) => 
            future::result(io::stdout().write_all(bytes))
                    .map(|_| state),
        Event::Server(OutputChunk::Exit(code)) => 
            std::process::exit(code),
    }
}

fn ok<T: 'static>(t: T) -> IOFuture<T> {
    Box::new(future::ok(t))
}

pub fn err(e: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, e)
}

impl From<ChildOutput> for OutputChunk {
    fn from(co: ChildOutput) -> Self {
        match co {
            ChildOutput::Stdout(bytes) => OutputChunk::Stdout(bytes),
            ChildOutput::Stderr(bytes) => OutputChunk::Stderr(bytes),
            ChildOutput::Exit(code) => OutputChunk::Exit(code),
        }
    }
}

type LoopFuture<C> = IOFuture<State<C>>;

type IOFuture<T> = Box<Future<Item = T, Error = io::Error>>;

///
///TODO: See https://users.rust-lang.org/t/why-cant-type-aliases-be-used-for-traits/10002/4
///
 #[cfg_attr(rustfmt, rustfmt_skip)]
trait ServerSink: Debug + Sink<SinkItem = OutputChunk, SinkError = io::Error> + 'static {}
 #[cfg_attr(rustfmt, rustfmt_skip)]
impl<T> ServerSink for T where T: Debug + Sink<SinkItem = OutputChunk, SinkError = io::Error> + 'static {}
