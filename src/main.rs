use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc, str::FromStr};

use boa_engine::{
    context::ContextBuilder,
    job::{Job, JobExecutor, NativeAsyncJob, PromiseJob},
    module::SimpleModuleLoader,
    optimizer::OptimizerOptions,
    Context, JsResult, Source,
};

#[derive(Default)]
struct Executor {
    promise_jobs: RefCell<VecDeque<PromiseJob>>,
    async_jobs: RefCell<VecDeque<NativeAsyncJob>>,
}

impl JobExecutor for Executor {
    fn enqueue_job(&self, job: Job, _: &mut Context) {
        match job {
            Job::PromiseJob(job) => self.promise_jobs.borrow_mut().push_back(job),
            Job::AsyncJob(job) => self.async_jobs.borrow_mut().push_back(job),
            job => eprintln!("unsupported job type {job:?}"),
        }
    }

    fn run_jobs(&self, context: &mut Context) -> JsResult<()> {
        loop {
            if self.promise_jobs.borrow().is_empty() && self.async_jobs.borrow().is_empty() {
                return Ok(());
            }

            let jobs = std::mem::take(&mut *self.promise_jobs.borrow_mut());
            for job in jobs {
                if let Err(e) = job.call(context) {
                    eprintln!("Uncaught {e}");
                }
            }

            let async_jobs = std::mem::take(&mut *self.async_jobs.borrow_mut());
            for async_job in async_jobs {
                if let Err(err) = pollster::block_on(async_job.call(&RefCell::new(context))) {
                    eprintln!("Uncaught {err}");
                }
                let jobs = std::mem::take(&mut *self.promise_jobs.borrow_mut());
                for job in jobs {
                    if let Err(e) = job.call(context) {
                        eprintln!("Uncaught {e}");
                    }
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let executor = Rc::new(Executor::default());
    let loader = Rc::new(SimpleModuleLoader::new(".").map_err(|e| anyhow::anyhow!(e.to_string()))?);

    let mut context = ContextBuilder::new()
        .job_executor(executor)
        .module_loader(loader.clone())
        .build()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    context.strict(false);

    boa_runtime::register(&mut context, boa_runtime::RegisterOptions::new())
        .expect("should not fail while registering the runtime");

    context.set_trace(false);

    let mut optimizer_options = OptimizerOptions::empty();
    optimizer_options.set(OptimizerOptions::STATISTICS, false);
    optimizer_options.set(OptimizerOptions::OPTIMIZE_ALL, false);
    context.set_optimizer_options(optimizer_options);

    let result = context
        .eval(Source::from_filepath(&PathBuf::from_str("test.js").unwrap()).unwrap())
        .map_err(|e| anyhow::Error::msg(format!("Failed to run script\n{}", e)))?;

    context
        .run_jobs()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let output = result
        .to_json(&mut context)
        .map_err(|e| anyhow::Error::msg(format!("Failed to convert output object.\n{}", e)))?;

    println!("{:#}", output);

    Ok(())
}
