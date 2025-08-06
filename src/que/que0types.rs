use std::sync::Arc;

use crate::{some_or_panic, trx::trx0trx::Trx, ut::ut0dbg::ut_ad};

/// Query Fork Types
const QUE_FORK_SELECT_NON_SCROLL: usize = 1;
const QUE_FORK_SELECT_SCROLL: usize = 2;
const QUE_FORK_INSERT: usize = 3;
const QUE_FORK_UPDATE: usize = 4;
const QUE_FORK_ROLLBACK: usize = 5;
const QUE_FORK_PURGE: usize = 6;
const QUE_FORK_EXECUTE: usize = 7;
const QUE_FORK_PROCEDURE: usize = 8;
const QUE_FORK_PROCEDURE_CALL: usize = 9;
const QUE_FORK_USER_INTERFACE: usize = 10;
const QUE_FORK_RECOVERY: usize = 11;

/// Query thread states
/// These states represent the various stages a query thread can be in during its lifecycle.
const QUE_THR_RUNNING: usize = 1;
const QUE_THR_PROCEDURE_WAIT: usize = 2;
const QUE_THR_COMPLETED: usize = 3;
const QUE_THR_COMMAND_WAIT: usize = 4;
const QUE_THR_LOCK_WAIT: usize = 5;
const QUE_THR_SIG_REPLY_WAIT: usize = 6;
const QUE_THR_SUSPENDED: usize = 7;
const QUE_THR_ERROR: usize = 8;

/// Query Fork States
const QUE_FORK_ACTIVE: usize = 1;
const QUE_FORK_COMMAND_WAIT: usize = 2;
const QUE_FORK_INVALID: usize = 3;
const QUE_FORK_BEING_FREED: usize = 4;

/// Query graph node types
const QUE_NODE_LOCK: usize = 1;
const QUE_NODE_INSERT: usize = 2;
const QUE_NODE_UPDATE: usize = 4;
const QUE_NODE_CURSOR: usize = 5;
const QUE_NODE_SELECT: usize = 6;
const QUE_NODE_AGGREGATE: usize = 7;
const QUE_NODE_FORK: usize = 8;
const QUE_NODE_THR: usize = 9;
const QUE_NODE_UNDO: usize = 10;
const QUE_NODE_COMMIT: usize = 11;
const QUE_NODE_ROLLBACK: usize = 12;
const QUE_NODE_PURGE: usize = 13;
const QUE_NODE_CREATE_TABLE: usize = 14;
const QUE_NODE_CREATE_INDEX: usize = 15;
const QUE_NODE_SYMBOL: usize = 16;
const QUE_NODE_RES_WORD: usize = 17;
const QUE_NODE_FUNC: usize = 18;
const QUE_NODE_ORDER: usize = 19;
const QUE_NODE_PROC: usize = 20;
const QUE_NODE_IF: usize = 21;
const QUE_NODE_WHILE: usize = 22;
const QUE_NODE_ASSIGNMENT: usize = 23;
const QUE_NODE_FETCH: usize = 24;
const QUE_NODE_OPEN: usize = 25;
const QUE_NODE_COL_ASSIGNMENT: usize = 26;
const QUE_NODE_FOR: usize = 27;
const QUE_NODE_RETURN: usize = 28;
const QUE_NODE_ROW_PRINTF: usize = 29;
const QUE_NODE_ELSE_IF: usize = 30;
const QUE_NODE_CALL: usize = 31;
const QUE_NODE_EXIT: usize = 32;

/// Query thread lock states
const QUE_THR_LOCK_NOLOCK: usize = 0;
const QUE_THR_LOCK_ROW: usize = 1;
const QUE_THR_LOCK_TABLE: usize = 2;

#[repr(u8)]
pub enum QueryForkType {
    /// Non-scrollable SELECT query fork.
    SelectNonScroll = QUE_FORK_SELECT_NON_SCROLL as u8,
    /// Scrollable SELECT query fork.
    SelectScroll = QUE_FORK_SELECT_SCROLL as u8,
    /// INSERT query fork.
    Insert = QUE_FORK_INSERT as u8,
    /// UPDATE query fork.
    Update = QUE_FORK_UPDATE as u8,
    /// ROLLBACK query fork.
    Rollback = QUE_FORK_ROLLBACK as u8,
    /// PURGE query fork.
    Purge = QUE_FORK_PURGE as u8,
    /// EXECUTE query fork.
    Execute = QUE_FORK_EXECUTE as u8,
    /// Stored procedure query fork.
    Procedure = QUE_FORK_PROCEDURE as u8,
    /// Stored procedure call query fork.
    ProcedureCall = QUE_FORK_PROCEDURE_CALL as u8,
    /// User interface query fork.
    UserInterface = QUE_FORK_USER_INTERFACE as u8,
    /// Recovery query fork.
    Recovery = QUE_FORK_RECOVERY as u8,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryThreadState {
    /// The query thread is running and processing commands.
    Running = QUE_THR_RUNNING as u8,
    /// The query thread is waiting for a procedure to complete.
    ProcedureWait = QUE_THR_PROCEDURE_WAIT as u8,
    /// The query thread has completed its execution.
    Completed = QUE_THR_COMPLETED as u8,
    /// The query thread is waiting for a command.
    CommandWait = QUE_THR_COMMAND_WAIT as u8,
    /// The query thread is waiting for a lock.
    LockWait = QUE_THR_LOCK_WAIT as u8,
    /// The query thread is waiting for a signal reply.
    SigReplyWait = QUE_THR_SIG_REPLY_WAIT as u8,
    /// The query thread is suspended.
    Suspended = QUE_THR_SUSPENDED as u8,
    /// The query thread encountered an error.
    Error = QUE_THR_ERROR as u8,
}

#[repr(u8)]
pub enum QueryThreadLockState {
    /// The query thread does not hold any locks.
    NoLock = QUE_THR_LOCK_NOLOCK as u8,
    /// The query thread holds a row lock.
    RowLock = QUE_THR_LOCK_ROW as u8,
    /// The query thread holds a table lock.
    TableLock = QUE_THR_LOCK_TABLE as u8,
}

#[repr(u8)]
pub enum QueryForkState {
    /// The query fork is active and processing.
    Active = QUE_FORK_ACTIVE as u8,
    /// The query fork is waiting for a command.
    CommandWait = QUE_FORK_COMMAND_WAIT as u8,
    /// The query fork is invalid, possibly due to an error.
    Invalid = QUE_FORK_INVALID as u8,
    /// The query fork is being freed.
    BeingFreed = QUE_FORK_BEING_FREED as u8,
}

#[repr(u16)]
pub enum QueryGraphNodeType {
    /// Lock node in the query graph.
    Lock = QUE_NODE_LOCK as u16,
    /// Insert node in the query graph.
    Insert = QUE_NODE_INSERT as u16,
    /// Update node in the query graph.
    Update = QUE_NODE_UPDATE as u16,
    /// Cursor node in the query graph.
    Cursor = QUE_NODE_CURSOR as u16,
    /// Select node in the query graph.
    Select = QUE_NODE_SELECT as u16,
    /// Aggregate node in the query graph.
    Aggregate = QUE_NODE_AGGREGATE as u16,
    /// Fork node in the query graph.
    Fork = QUE_NODE_FORK as u16,
    /// Thread node in the query graph.
    Thread = QUE_NODE_THR as u16,
    /// Undo the node in the query graph.
    Undo = QUE_NODE_UNDO as u16,
    /// Commit node in the query graph.
    Commit = QUE_NODE_COMMIT as u16,
    /// Rollback node in the query graph.
    Rollback = QUE_NODE_ROLLBACK as u16,
    /// Purge the node in the query graph.
    Purge = QUE_NODE_PURGE as u16,
    /// Create a table node in the query graph.
    CreateTable = QUE_NODE_CREATE_TABLE as u16,
    /// Create the index node in the query graph.
    CreateIndex = QUE_NODE_CREATE_INDEX as u16,
    /// Symbol node in the query graph.
    Symbol = QUE_NODE_SYMBOL as u16,
    /// Reserved word node in the query graph.
    ResWord = QUE_NODE_RES_WORD as u16,
    /// Function node in the query graph.
    Func = QUE_NODE_FUNC as u16,
    /// Order by clause node in the query graph.
    Order = QUE_NODE_ORDER as u16,
    /// Procedure node in the query graph.
    Procedure = QUE_NODE_PROC as u16,
    /// If statement node in the query graph.
    If = QUE_NODE_IF as u16,
    /// While loop node in the query graph.
    While = QUE_NODE_WHILE as u16,
    /// Assignment statement node in the query graph.
    Assignment = QUE_NODE_ASSIGNMENT as u16,
    /// Fetch the statement node in the query graph.
    Fetch = QUE_NODE_FETCH as u16,
    /// Open cursor statement node in the query graph.
    Open = QUE_NODE_OPEN as u16,
    /// Column assignment statement node in the query graph
    ColAssignment = QUE_NODE_COL_ASSIGNMENT as u16,
    /// For loop node in the query graph.
    For = QUE_NODE_FOR as u16,
    /// Return the statement node in the query graph.
    Return = QUE_NODE_RETURN as u16,
    /// Row printf node in the query graph.
    RowPrintf = QUE_NODE_ROW_PRINTF as u16,
    /// Else if the statement node in the query graph.
    ElseIf = QUE_NODE_ELSE_IF as u16,
    /// Call the statement node in the query graph.
    Call = QUE_NODE_CALL as u16,
    /// Exit statement node in the query graph.
    Exit = QUE_NODE_EXIT as u16,
}

pub struct QueThr {
    query_thread_state: QueryThreadState,
    /// Reference to the fork node of the query thread.
    graph: Arc<QueFork>,
    /// If the query thread is active.
    is_active: bool,
    /// List of thread nodes of the fork node.
    thr_nodes: Vec<Arc<QueThr>>,
    /// List of threads in the wait list of the trx.
    wait_list: Vec<Arc<QueThr>>,
    /// list of runnable thread nodes
    thr_runnable: Vec<Arc<QueThr>>,
    /// Resource usage of the query thread.
    resource: usize,
    run_node: Option<Arc<QueFork>>,
}

pub struct QueFork {
    gra_node_type: QueryGraphNodeType,
    /// The type of the query fork.
    que_type: QueryForkType,
    /// If this is the root of a graph, the number of query threads that have been started in the que_thr_move_to_run state.
    n_active_threads: usize,
    /// Reference to the transaction object.
    trx: Arc<Trx>,
}

impl QueThr {
    pub fn new(graph: Arc<QueFork>) -> Self {
        Self {
            query_thread_state: QueryThreadState::Running,
            graph,
            is_active: true,
            thr_nodes: Vec::new(),
            wait_list: Vec::new(),
            thr_runnable: Vec::new(),
            resource: 0,
            run_node: None,
        }
    }

    pub fn thr_get_trx(&self) -> Arc<Trx> {
        self.graph.trx.clone()
    }

    pub fn que_thr_step(&mut self) -> Result<(), String> {
        let trx = self.thr_get_trx();

        ut_ad!(self.query_thread_state == QueryThreadState::Running);
        self.resource += 1;

        match some_or_panic!(&self.run_node).gra_node_type {
            QueryGraphNodeType::Lock => {}
            QueryGraphNodeType::Insert => {}
            QueryGraphNodeType::Update => {}
            QueryGraphNodeType::Cursor => {}
            QueryGraphNodeType::Select => {}
            QueryGraphNodeType::Aggregate => {}
            QueryGraphNodeType::Fork => {}
            QueryGraphNodeType::Thread => {}
            QueryGraphNodeType::Undo => {}
            QueryGraphNodeType::Commit => {}
            QueryGraphNodeType::Rollback => {}
            QueryGraphNodeType::Purge => {}
            QueryGraphNodeType::CreateTable => {}
            QueryGraphNodeType::CreateIndex => {}
            QueryGraphNodeType::Symbol => {}
            QueryGraphNodeType::ResWord => {}
            QueryGraphNodeType::Func => {}
            QueryGraphNodeType::Order => {}
            QueryGraphNodeType::Procedure => {}
            QueryGraphNodeType::If => {}
            QueryGraphNodeType::While => {}
            QueryGraphNodeType::Assignment => {}
            QueryGraphNodeType::Fetch => {}
            QueryGraphNodeType::Open => {}
            QueryGraphNodeType::ColAssignment => {}
            QueryGraphNodeType::For => {}
            QueryGraphNodeType::Return => {}
            QueryGraphNodeType::RowPrintf => {}
            QueryGraphNodeType::ElseIf => {}
            QueryGraphNodeType::Call => {}
            QueryGraphNodeType::Exit => {}
        }

        Ok(())
    }
}
