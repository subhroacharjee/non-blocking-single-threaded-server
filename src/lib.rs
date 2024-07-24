use libc::{pollfd, POLLIN, POLLRDNORM};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::{BufRead, BufReader, Result},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
    rc::Rc,
    usize, vec,
};

#[derive(Debug)]
pub struct ClientStream {
    pub client: TcpStream,
    pub is_handled: bool,
}

// copied
macro_rules! syscall {
    ($fn: ident ( $($arg: expr),* $(,)* ) ) => {{
        let res = unsafe { libc::$fn($($arg, )*) };
        if res == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res)
        }
    }};
}

pub struct BlockingTcpListener {
    listener: TcpListener,
    poll_fds: Rc<RefCell<Vec<pollfd>>>,
    handlers: Rc<RefCell<HashMap<i32, Box<dyn Fn()>>>>,
    clients: Rc<RefCell<Vec<TcpStream>>>,
    new_clients: Rc<RefCell<Vec<TcpStream>>>,
    remove_fds: Rc<RefCell<Vec<i32>>>,
}

impl BlockingTcpListener {
    pub fn new(addr: String) -> BlockingTcpListener {
        let listener = TcpListener::bind(addr).unwrap();
        let fd = listener.as_raw_fd();
        BlockingTcpListener {
            listener,
            poll_fds: Rc::new(RefCell::new(vec![pollfd {
                fd,
                events: POLLIN,
                revents: 0,
            }])),
            handlers: Rc::new(RefCell::new(HashMap::new())),
            clients: Rc::new(RefCell::new(vec![])),
            new_clients: Rc::new(RefCell::new(vec![])),
            remove_fds: Rc::new(RefCell::new(vec![])),
        }
    }

    pub fn initalize(&self) {
        let fd = self.listener.as_raw_fd();

        let new_clients = Rc::clone(&self.new_clients);
        let clients = Rc::clone(&self.clients);
        let listener = self.listener.try_clone().unwrap();
        let handlers = Rc::clone(&self.handlers);
        handlers.try_borrow_mut().unwrap().insert(
            fd,
            Box::new(move || {
                let (client, addr) = listener.accept().unwrap();

                println!("{} is connected to the server", addr);
                new_clients
                    .try_borrow_mut()
                    .unwrap()
                    .push(client.try_clone().unwrap());

                clients
                    .try_borrow_mut()
                    .unwrap()
                    .push(client.try_clone().unwrap());
            }),
        );
    }

    pub fn wait(&mut self) -> Result<usize> {
        let fds = Rc::clone(&self.poll_fds);
        loop {
            let poll_fds = Rc::clone(&self.poll_fds);

            let len_of_fds = fds.try_borrow().unwrap().len() as libc::nfds_t;
            match syscall!(poll(
                poll_fds.try_borrow_mut().unwrap().as_mut_ptr(),
                len_of_fds,
                -1
            )) {
                Ok(n) => break Ok(n as usize),
                Err(e) if e.raw_os_error() == Some(libc::EAGAIN) => continue,
                Err(e) => return Err(e),
            };
        }
    }

    pub fn event_loop(&mut self) -> ! {
        loop {
            let no_of_event = self.wait().unwrap();

            let poll_fds = Rc::clone(&self.poll_fds);
            let handlers = Rc::clone(&self.handlers);
            if no_of_event > 0 {
                for poll_fd in poll_fds.try_borrow().unwrap().iter() {
                    if poll_fd.revents & POLLIN != 0 {
                        if let Some(handler) = handlers.try_borrow().unwrap().get(&poll_fd.fd) {
                            handler();
                        }
                    } else if poll_fd.revents & POLLRDNORM != 0 {
                        if let Some(handler) = handlers.try_borrow().unwrap().get(&poll_fd.fd) {
                            handler();
                        }
                    }
                }

                self.handle_new_clients();

                // remove dead clients
                for fd in self.remove_fds.try_borrow().unwrap().iter() {
                    let index = poll_fds
                        .try_borrow_mut()
                        .unwrap()
                        .iter()
                        .position(|x_fd| *fd == x_fd.fd)
                        .unwrap();
                    poll_fds.try_borrow_mut().unwrap().remove(index);

                    handlers.try_borrow_mut().unwrap().remove(fd);
                }

                self.remove_fds.try_borrow_mut().unwrap().clear();
            }
        }
    }

    fn handle_new_clients(&mut self) {
        let len_of_new_clients = self.new_clients.try_borrow().unwrap().len();
        if len_of_new_clients > 0 {
            for tmp_new_client in self.new_clients.try_borrow_mut().unwrap().iter() {
                let poll_fds = Rc::clone(&self.poll_fds);
                let handlers = Rc::clone(&self.handlers);
                let clients = Rc::clone(&self.clients);
                let dead_fds = Rc::clone(&self.remove_fds);
                let new_client = tmp_new_client.try_clone().unwrap();
                let fd = new_client.as_raw_fd();
                let idx_of_client = clients
                    .try_borrow()
                    .unwrap()
                    .iter()
                    .position(|clx| {
                        *clx.peer_addr().unwrap().to_string()
                            == new_client.peer_addr().unwrap().to_string()
                    })
                    .unwrap();
                println!("{}", idx_of_client);
                poll_fds.try_borrow_mut().unwrap().push(pollfd {
                    fd,
                    events: POLLRDNORM,
                    revents: 0,
                });

                handlers.try_borrow_mut().unwrap().insert(
                    fd,
                    Box::new(move || {
                        let fd = new_client.as_raw_fd();
                        let addr = new_client.peer_addr().unwrap().to_string();
                        let mut buf_reader = BufReader::new(new_client.try_clone().unwrap());
                        let mut buf = String::new();
                        if match buf_reader.read_line(&mut buf) {
                            Ok(size) => {
                                let mut return_val = true;
                                if size > 0 {
                                    print!("{}> {}", addr, buf);
                                    return_val = false;
                                }
                                return_val
                            }
                            Err(_err) => {
                                // removing the client
                                true
                            }
                        } {
                            dead_fds.try_borrow_mut().unwrap().push(fd);
                            clients.try_borrow_mut().unwrap().remove(idx_of_client);
                        }
                    }),
                );
            }

            self.new_clients.try_borrow_mut().unwrap().clear();
        }
    }
}
