use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use boa_engine::{
    context::ContextBuilder,
    job::{FutureJob, JobQueue, NativeJob},
    module::SimpleModuleLoader,
    optimizer::OptimizerOptions,
    Context, JsValue, Source,
};

use serde_json::json;

#[derive(Default)]
pub struct SimpleJobQueue(RefCell<VecDeque<NativeJob>>);

impl std::fmt::Debug for SimpleJobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SimpleQueue").field(&"..").finish()
    }
}

impl SimpleJobQueue {
    /// Creates an empty `SimpleJobQueue`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl JobQueue for SimpleJobQueue {
    fn enqueue_promise_job(&self, job: NativeJob, _: &mut Context) {
        self.0.borrow_mut().push_back(job);
    }

    fn run_jobs(&self, context: &mut Context) {
        let mut next_job = self.0.borrow_mut().pop_front();
        while let Some(job) = next_job {
            if job.call(context).is_err() {
                self.0.borrow_mut().clear();
                return;
            };
            next_job = self.0.borrow_mut().pop_front();
        }
    }

    fn enqueue_future_job(&self, future: FutureJob, context: &mut Context) {
        let job = pollster::block_on(future);
        self.enqueue_promise_job(job, context);
    }
}

fn main() -> anyhow::Result<()> {
    let executor = Rc::new(SimpleJobQueue::default());
    let loader = Rc::new(SimpleModuleLoader::new(".").map_err(|e| anyhow::anyhow!(e.to_string()))?);

    let mut context = ContextBuilder::new()
        .job_queue(executor)
        .module_loader(loader.clone())
        .build()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    context.strict(false);

    boa_runtime::register(&mut context, boa_runtime::RegisterOptions::new())
        .expect("should not fail while registering the runtime");

    let mut optimizer_options = OptimizerOptions::empty();
    optimizer_options.set(OptimizerOptions::STATISTICS, false);
    optimizer_options.set(OptimizerOptions::OPTIMIZE_ALL, false);
    context.set_optimizer_options(optimizer_options);

    let script = get_script_complex();

    let result = context
        .eval(Source::from_bytes(script))
        .map_err(|e| anyhow::Error::msg(format!("Failed to run script\n{}", e)))?;

    context.run_jobs();

    let output = convert_output(&mut context, result)?;

    println!("{:#}", output);

    Ok(())
}

fn convert_output(
    context: &mut Context,
    last_result: JsValue,
) -> anyhow::Result<serde_json::Value> {
    match last_result {
        JsValue::Undefined => Ok(json!(null)),
        _ => last_result
            .to_json(context)
            .map_err(|e| anyhow::Error::msg(format!("Failed to convert output object.\n{}", e))),
    }
}

// fn get_script_simple() -> &'static str {
//     r##"
//     x = {
//       "hello": "world"
//     };
//     x;
//     "##
// }

fn get_script_complex() -> &'static str {
    include_str!("../test.js")
}
